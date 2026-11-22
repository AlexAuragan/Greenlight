//! Cross-session busy feedback uses local fixtures, never provider calls.
use super::*;
use ratatui::{backend::TestBackend, Terminal};
fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[test]
fn busy_project_keeps_step_drafts_and_offers_owner_navigation_without_stealing_help_input() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let a = store.create("demo", "Owning workflow", "work").unwrap();
    let b = store.create("demo", "Selected workflow", "work").unwrap();
    let owner = store.claim_execution(&a.id, None).unwrap();
    let mut app = App::new(Some(&b.id));
    app.refresh(&store).unwrap();
    assert!(screen(&app).contains("Project busy"));
    assert!(app
        .footer_actions()
        .contains(&shell::Action::OpenProjectOwner));
    assert!(app
        .footer_actions()
        .contains(&shell::Action::RefreshProject));
    app.composer = "Keep these original instructions".into();
    app.resume_instruction(root.path(), &store, app.composer.clone());
    assert_eq!(app.composer, "Keep these original instructions");
    assert!(app.runner.is_none());
    assert!(app.notice.contains("Project busy"));
    assert!(!store
        .history(&b)
        .unwrap()
        .iter()
        .any(|e| e.2 == "user.instruction"));
    app.open_help();
    app.on_key(
        KeyEvent::new(KeyCode::Char('O'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.session().unwrap().id, b.id);
    app.on_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    app.activate(shell::Action::OpenProjectOwner, root.path(), &store)
        .unwrap();
    assert_eq!(app.session().unwrap().id, a.id);
    assert!(app.runner.is_none());
    store.finish_execution(&a.id, &owner, None).unwrap();
}
#[test]
fn creating_while_busy_retains_the_goal_as_an_idle_session_and_never_starts_a_worker() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("workflows/demo");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        root.path().join("greenlight.yaml"),
        "provider:\n  model: fixture\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("work.md"),
        "---\nid: work\nverification:\n  type: none\n---\nWork\n",
    )
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    let a = store.create("demo", "Owner", "work").unwrap();
    let owner = store.claim_execution(&a.id, None).unwrap();
    let mut app = App::new(Some(&a.id));
    app.refresh(&store).unwrap();
    app.new_form(root.path()).unwrap();
    app.new_session.as_mut().unwrap().goal = Some("My saved goal".into());
    app.submit_new_session(root.path(), &store).unwrap();
    let new = app.session().unwrap();
    assert_ne!(new.id, a.id);
    assert_eq!(new.goal, "My saved goal");
    assert_eq!(new.status, "ready");
    assert!(app.notice.contains("Created; not started"));
    assert!(app.runner.is_none());
    assert_eq!(
        store
            .history(new)
            .unwrap()
            .iter()
            .filter(|e| e.2 == "runner.started")
            .count(),
        0
    );
    assert!(app.other_project_owner());
    store.finish_execution(&a.id, &owner, None).unwrap();
    drop(owner);
    app.activate(shell::Action::RefreshProject, root.path(), &store)
        .unwrap();
    assert!(!app.other_project_owner());
    assert!(app.runner.is_none()); // refresh never queues or starts work
    assert!(app.notice.contains("Start explicitly"));
}
#[test]
fn interrupted_maintenance_is_visible_and_can_be_recovered_from_its_owner_session() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let a = store.create("demo", "Interrupted control", "work").unwrap();
    let b = store.create("demo", "Other workflow", "work").unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<()> = store.with_project_control(&a, "restore", |_| panic!("fixture crash"));
    }))
    .is_err());
    let mut app = App::new(Some(&b.id));
    app.refresh(&store).unwrap();
    assert!(screen(&app).contains("Project interrupted"));
    app.activate(shell::Action::OpenProjectOwner, root.path(), &store)
        .unwrap();
    assert_eq!(app.session().unwrap().id, a.id);
    assert!(app
        .workspace
        .as_ref()
        .is_some_and(|w| w.primary == crate::workspace_view::PrimaryAction::RecoverOwner));
    app.on_key(
        KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(app.notice.contains("Project execution interrupted"));
    app.activate(shell::Action::RecoverOwner, root.path(), &store)
        .unwrap();
    assert!(store.project_status().unwrap().is_none());
    assert!(app.runner.is_none());
}

#[test]
fn claim_race_returns_typed_busy_feedback_and_restores_the_unsent_instruction() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let a = store.create("demo", "winner", "work").unwrap();
    let b = store.create("demo", "loser", "work").unwrap();
    let mut app = App::new(Some(&b.id));
    app.refresh(&store).unwrap();
    let text = "Original instruction that lost the claim race".to_owned();
    app.compose.last_instruction = Some((app.compose.scope.clone(), text.clone()));
    app.composer.clear();
    let owner = store.claim_execution(&a.id, None).unwrap();
    let error = store.claim_execution(&b.id, Some(&text)).err().unwrap();
    let (finished_tx, finished) = std::sync::mpsc::channel();
    finished_tx
        .send(Err(crate::tui_actions::RunnerError::from(error)))
        .unwrap();
    app.runner = Some(Runner {
        tool_rerun: false,
        live: None,
        cancel_retry: None,
        session_id: Some(b.id.clone()),
        prompts: std::sync::mpsc::channel().1,
        answers: std::sync::mpsc::channel().0,
        finished,
    });
    app.tick();
    assert!(app.runner.is_none());
    assert_eq!(app.composer, text);
    assert!(app.notice.contains("Not started"));
    assert!(app.notice.contains("retained as a draft"));
    assert!(app.diagnostic.is_none());
    assert!(store
        .history(&b)
        .unwrap()
        .iter()
        .all(|e| e.2 != "user.instruction"));
    store.finish_execution(&a.id, &owner, None).unwrap();
}

#[test]
fn blocker_arriving_during_refresh_cannot_leave_old_rows_tagged_as_current() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let external = Store::open(root.path()).unwrap();
    let mut session = store.create("demo", "goal", "work").unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    let blocker = external
        .event(&session, Some("work"), "step.blocked", "evidence missing")
        .unwrap();
    session.status = "blocked".into();
    external.save(&session).unwrap();
    // Reproduce the old mid-refresh ordering: rows came from the earlier read,
    // but the generation was stamped after the worker's final commit.
    app.activity_generation.as_mut().unwrap().revision = store.data_revision().unwrap();
    assert!(!app.events.iter().any(|e| e.id == blocker));
    app.refresh(&store).unwrap();
    assert!(app.events.iter().any(|e| e.id == blocker));
    assert!(app
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("evidence missing"));
}
