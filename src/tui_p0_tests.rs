//! Engine-backed UI acceptance tests with an 80×24 terminal buffer.
use super::*;
use crate::{engine::Engine, provider::Script};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    fs,
    sync::{mpsc, Arc, Mutex},
    thread,
};

fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

fn key(app: &mut App, store: &Store, root: &Path, code: KeyCode) {
    assert!(!app
        .on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
        .unwrap());
}

fn wait(app: &mut App, store: &Store, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate(app) {
        assert!(
            Instant::now() < deadline,
            "UI did not reach expected state: {}",
            app.notice
        );
        app.tick();
        app.refresh(store).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
}

fn start(app: &mut App, root: &Path, session: &str, instruction: &str, responses: &[&str]) {
    let mut engine = Engine::open(root).unwrap();
    engine.tools.approvals = crate::approvals::ApprovalStore::isolated(root).unwrap();
    engine.provider.script = Some(Arc::new(Mutex::new(Script {
        responses: responses.iter().map(|text| Ok((*text).into())).collect(),
        requests: Vec::new(),
        ..Default::default()
    })));
    let (prompts_tx, prompts) = mpsc::channel();
    let (answers, answer_rx) = mpsc::channel();
    let (finished_tx, finished) = mpsc::channel();
    let session = session.to_string();
    let instruction = instruction.to_string();
    engine.tools.use_tui(prompts_tx, answer_rx);
    thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(engine.run(
            &session,
            Some(&instruction),
            None,
        ));
        let _ = finished_tx.send(result.map_err(|error| format!("{error:#}")));
    });
    app.runner = Some(Runner {
        prompts,
        answers,
        finished,
    });
}

#[test]
fn confirmation_is_visible_at_80x24_and_rejects_enter_repeat_and_global_shortcuts() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    let (_pt, prompts) = mpsc::channel();
    let (answers, received) = mpsc::channel();
    let (_ft, finished) = mpsc::channel();
    app.runner = Some(Runner {
        prompts,
        answers,
        finished,
    });
    app.approval=Some(ApprovalPrompt {content:"ALL native writes in ALL projects\nFuture matching requests may skip prompts.\n[y] save / [n] cancel".into(),mode:ApprovalMode::ConfirmPersistent});
    let rendered = screen(&app);
    assert!(rendered.contains("Confirm persistent grant"));
    assert!(rendered.contains("Save grant and allow"));
    key(&mut app, &store, root.path(), KeyCode::Enter);
    key(&mut app, &store, root.path(), KeyCode::Char('g'));
    let repeated =
        KeyEvent::new_with_kind(KeyCode::Char('y'), KeyModifiers::NONE, KeyEventKind::Repeat);
    app.on_key(repeated, root.path(), &store).unwrap();
    assert!(received.try_recv().is_err());
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert_eq!(received.recv().unwrap(), "n");
}

#[test]
fn workflow_at_80x24_covers_deny_inspect_instruct_verify_and_recover() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("workflows/demo");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        root.path().join("greenlight.yaml"),
        "provider:\n  model: fixture\n",
    )
    .unwrap();
    fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: work\nsteps: [work, gate]\n",
    )
    .unwrap();
    fs::write(folder.join("work.md"),"---\nid: work\ntools: [write]\noutputs:\n  required: [report.md]\nverification:\n  type: agent\n  prompt: reviewer.md\ntransitions:\n  success: gate\n---\nDo the work\n").unwrap();
    fs::write(folder.join("gate.md"),"---\nid: gate\nkind: human\ninputs:\n  required: [report.md]\nverification:\n  type: none\n---\nReview the result\n").unwrap();
    fs::write(
        folder.join("reviewer.md"),
        "Require supported test evidence",
    )
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("demo", "fixture", "work").unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    let write = "action: tool\ntool: write\npath: result.txt\ncontent: implemented";
    start(
        &mut app,
        root.path(),
        &session.id,
        "initial",
        &[
            write,
            write,
            write,
            "action: artifact\nname: report.md\ncontent: Initial report",
            "action: finish",
            "passed: false\nreason: evidence missing",
        ],
    );
    wait(&mut app, &store, |a| a.approval.is_some());
    assert!(screen(&app).contains("Tool permission required"));
    key(&mut app, &store, root.path(), KeyCode::Enter);
    assert!(!root.path().join("result.txt").exists());
    key(&mut app, &store, root.path(), KeyCode::Char('n'));
    wait(&mut app, &store, |a| a.approval.is_some());
    assert!(!root.path().join("result.txt").exists());
    key(&mut app, &store, root.path(), KeyCode::Char('g'));
    wait(&mut app, &store, |a| {
        a.approval
            .as_ref()
            .is_some_and(|p| p.mode == ApprovalMode::ConfirmPersistent)
    });
    assert!(screen(&app).contains("Save grant and allow"));
    key(&mut app, &store, root.path(), KeyCode::Enter);
    key(&mut app, &store, root.path(), KeyCode::Char('n'));
    wait(&mut app, &store, |a| {
        a.approval
            .as_ref()
            .is_some_and(|p| p.mode == ApprovalMode::Persistent)
    });
    assert!(!root.path().join("result.txt").exists());
    assert!(crate::approvals::ApprovalStore::isolated(root.path())
        .unwrap()
        .matched_scope("write", "")
        .is_none());
    key(&mut app, &store, root.path(), KeyCode::Char('y'));
    wait(&mut app, &store, |a| a.runner.is_none());
    app.refresh(&store).unwrap();
    assert_eq!(store.session(&session.id).unwrap().status, "blocked");
    assert_eq!(
        fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "implemented"
    );
    assert!(screen(&app).contains("ATTENTION"));
    assert!(app
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("evidence missing"));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    app.view = View::Artifacts;
    app.focus = Focus::Artifacts;
    assert!(screen(&app).contains("Initial report"));
    start(
        &mut app,
        root.path(),
        &session.id,
        "Add the missing verification evidence",
        &[
            "action: artifact\nname: report.md\ncontent: Verified report",
            "action: finish",
            "passed: true\nreason: supported",
        ],
    );
    wait(&mut app, &store, |a| {
        a.approval
            .as_ref()
            .is_some_and(|p| p.mode == ApprovalMode::Human)
    });
    assert!(screen(&app).contains("Human approval required"));
    assert!(app
        .approval
        .as_ref()
        .unwrap()
        .content
        .contains("Verified report"));
    key(&mut app, &store, root.path(), KeyCode::Char('n'));
    wait(&mut app, &store, |a| a.runner.is_none());
    app.refresh(&store).unwrap();
    assert_eq!(store.session(&session.id).unwrap().current_step, "gate");
    if app.diagnostic.is_some() {
        key(&mut app, &store, root.path(), KeyCode::Esc);
    }

    // Abrupt worker loss: its owner record remains, the channels disconnect,
    // and no pending answer may be replayed to the recovered worker.
    let owner = store.claim_execution(&session.id, None).unwrap();
    let (pt, prompts) = mpsc::channel();
    let (answers, old_answers) = mpsc::channel();
    let (ft, finished) = mpsc::channel();
    pt.send(ApprovalPrompt {
        content: "Unanswered checkpoint".into(),
        mode: ApprovalMode::Human,
    })
    .unwrap();
    app.runner = Some(Runner {
        prompts,
        answers,
        finished,
    });
    app.tick();
    assert!(app.approval.is_some());
    drop(owner);
    drop(pt);
    drop(ft);
    app.tick();
    assert!(app.approval.is_none() && app.runner.is_none());
    assert!(screen(&app).contains("ATTENTION"));
    assert!(old_answers.try_recv().is_err());
    key(&mut app, &store, root.path(), KeyCode::Esc);
    app.composer = "/recover".into();
    app.submit(root.path(), &store);
    app.refresh(&store).unwrap();
    assert_eq!(store.session(&session.id).unwrap().status, "blocked");
    if app.diagnostic.is_some() {
        key(&mut app, &store, root.path(), KeyCode::Esc);
    }
    start(
        &mut app,
        root.path(),
        &session.id,
        "Resume after inspection",
        &[],
    );
    wait(&mut app, &store, |a| a.approval.is_some());
    key(&mut app, &store, root.path(), KeyCode::Char('y'));
    wait(&mut app, &store, |a| a.runner.is_none());
    app.refresh(&store).unwrap();
    assert_eq!(store.session(&session.id).unwrap().status, "completed");
    assert!(app.diagnostic.is_none());
    assert!(screen(&app).contains("COMPLETED"));
    let history = store.history(&store.session(&session.id).unwrap()).unwrap();
    assert!(history.iter().any(
        |(_, _, k, d)| k == "user.instruction" && d == "Add the missing verification evidence"
    ));
    assert!(history
        .iter()
        .any(|(_, _, k, d)| k == "permission.decision" && d.contains("denied by user")));
    assert!(history.iter().any(|(_, _, k, _)| k == "runner.recovered"));
    let usage = store
        .usage_report(
            &store.session(&session.id).unwrap(),
            crate::usage::Scope::Session,
        )
        .unwrap();
    assert_eq!(usage.totals.requests, 9);
    assert_eq!(usage.totals.input.reported, 0);
    assert!(usage.totals.input.estimated > 0);
    app.composer = "/usage".into();
    app.submit(root.path(), &store);
    assert!(screen(&app).contains("Usage — session spending"));
    assert!(app.usage_detail.as_ref().unwrap().contains("unknown"));
    key(&mut app, &store, root.path(), KeyCode::Down);
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 1);
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert!(app.usage_detail.is_none());
}

#[test]
fn malformed_events_and_missing_artifacts_remain_inspectable() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("demo", "goal", "step").unwrap();
    store
        .event(&session, Some("step"), "context.inputs", "{broken-json")
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    assert!(screen(&app).contains("ATTENTION"));
    assert!(app
        .events
        .iter()
        .any(|e| e.kind == "event.invalid" && e.data.contains("{broken-json")));
    key(&mut app, &store, root.path(), KeyCode::Down);
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 1);
    key(&mut app, &store, root.path(), KeyCode::Esc);
    app.refresh(&store).unwrap();
    assert!(app.diagnostic.is_none());
    store
        .add_artifact(&session, "step", "report.md", "contents")
        .unwrap();
    let blob = fs::read_dir(store.directory.join("blobs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_file(blob).unwrap();
    app.refresh(&store).unwrap();
    assert!(app
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("Artifact loading failed"));
    assert!(!app.events.is_empty());
}

#[test]
fn partial_restore_is_reported_and_does_not_fork_or_claim_success() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut session = store.create("demo", "goal", "edit").unwrap();
    let checkpoint = store.history(&session).unwrap()[0].0;
    for name in ["a.txt", "b.txt"] {
        store
            .add_change(&session, "edit", name, Some("old"), "new")
            .unwrap();
        fs::write(root.path().join(name), "new").unwrap();
    }
    session.status = "blocked".into();
    store.save(&session).unwrap();
    store.db.execute_batch("CREATE TRIGGER reject_second_restore BEFORE INSERT ON restore_changes WHEN NEW.path='b.txt' BEGIN SELECT RAISE(ABORT,'injected journal failure'); END;").unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.restart_choices = vec![RestartCandidate {
        step: "edit".into(),
        prior_event: checkpoint,
    }];
    app.prepare_restart(root.path(), &store);
    key(&mut app, &store, root.path(), KeyCode::Char('n'));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("b.txt")).unwrap(),
        "new"
    );
    app.restart_choices = vec![RestartCandidate {
        step: "edit".into(),
        prior_event: checkpoint,
    }];
    app.prepare_restart(root.path(), &store);
    app.confirm_restart(root.path(), &store);
    let message = app.diagnostic.as_ref().unwrap();
    assert!(message.contains("2 file(s) changed (1 journaled)"));
    assert!(screen(&app).contains("ATTENTION"));
    assert_eq!(
        store.session(&session.id).unwrap().active_branch,
        session.active_branch
    );
    assert_eq!(store.workspace_edits(&session).unwrap().len(), 3);
    let history = store.history(&session).unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|(_, _, k, _)| k == "workspace.restored")
            .count(),
        1
    );
    assert!(history
        .iter()
        .any(|(_, _, k, _)| k == "workspace.restore-failed"));
    assert!(!history.iter().any(|(_, _, k, _)| k == "branch.forked"));
}
