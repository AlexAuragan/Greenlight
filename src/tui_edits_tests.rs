use super::*;
use std::fs;
fn key(app: &mut App, root: &Path, store: &Store, code: KeyCode) {
    assert!(!app
        .on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
        .unwrap());
}
fn setup() -> (tempfile::TempDir, Store, Session, App) {
    let root = tempfile::tempdir().unwrap();
    crate::project::init(root.path()).unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store
        .create("optimize-performance", "Make the bar faster", "evaluate")
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    (root, store, session, app)
}
fn edit(
    root: &Path,
    store: &Store,
    session: &Session,
    path: &str,
    before: Option<&str>,
    after: &str,
) {
    fs::write(root.join(path), after).unwrap();
    store
        .add_change(session, "optimize", path, before, after)
        .unwrap();
}
#[test]
fn result_opens_with_reported_failure_and_no_checks_not_a_claim_of_improvement() {
    let (root, store, mut session, mut app) = setup();
    edit(
        root.path(),
        &store,
        &session,
        "bar.py",
        Some("user changes before run\n"),
        "candidate optimization\n",
    );
    store
        .event(&session, Some("evaluate"), "step.started", "")
        .unwrap();
    store
        .add_artifact(
            &session,
            "evaluate",
            "performance-evaluation.md",
            "Status: measurement_failure\nUser must run the benchmark; improvement is unverified.",
        )
        .unwrap();
    store
        .event(&session, Some("evaluate"), "verification.none", "")
        .unwrap();
    store
        .event(&session, Some("evaluate"), "verification.passed", "")
        .unwrap();
    store
        .event(
            &session,
            Some("evaluate"),
            "step.completed",
            "Status: measurement_failure. No measured improvement established.",
        )
        .unwrap();
    store
        .event(&session, None, "session.completed", "")
        .unwrap();
    session.status = "completed".into();
    store.save(&session).unwrap();
    app.refresh(&store).unwrap();
    assert!(app.result_open);
    assert!(app.result_text.contains("measurement_failure"));
    assert!(app.result_text.contains("no checks required"));
    assert!(app.result_text.contains("Source edits: 1 file"));
    // A replacement report must not rewrite the evidence for the completed run.
    store
        .add_artifact(
            &session,
            "evaluate",
            "performance-evaluation.md",
            "LATER REPLACEMENT SUCCESS",
        )
        .unwrap();
    app.refresh(&store).unwrap();
    assert!(!app.result_text.contains("LATER REPLACEMENT SUCCESS"));
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert!(!app.result_open);
    app.refresh(&store).unwrap();
    assert!(!app.result_open);
    key(&mut app, root.path(), &store, KeyCode::Char('s'));
    assert!(app.result_open);
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    assert_eq!(app.view, View::Edits);
    assert!(!app.result_open);
    assert!(app.edits.text.contains("-user changes before run"));
    assert!(app.edits.text.contains("+candidate optimization"));
}
#[test]
fn file_then_all_revert_preserves_initial_manual_work_and_records_restores() {
    let (root, store, session, mut app) = setup();
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("preexisting manual draft\n"),
        "first model edit\n",
    );
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("first model edit\n"),
        "latest model edit\n",
    );
    edit(
        root.path(),
        &store,
        &session,
        "new.txt",
        None,
        "new model file\n",
    );
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    assert_eq!(app.edits.files.len(), 2);
    key(&mut app, root.path(), &store, KeyCode::Char('u'));
    assert!(app.edits.pending.is_some());
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "latest model edit\n"
    );
    let repeat =
        KeyEvent::new_with_kind(KeyCode::Char('y'), KeyModifiers::NONE, KeyEventKind::Repeat);
    app.on_key(repeat, root.path(), &store).unwrap();
    assert!(app.edits.pending.is_some());
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert!(app.edits.pending.is_none());
    key(&mut app, root.path(), &store, KeyCode::Char('u'));
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "preexisting manual draft\n"
    );
    assert!(root.path().join("new.txt").exists());
    key(&mut app, root.path(), &store, KeyCode::Char('U'));
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert!(!root.path().join("new.txt").exists());
    assert_eq!(
        store.session(&session.id).unwrap().active_branch,
        session.active_branch
    );
    assert_eq!(
        store
            .history(&session)
            .unwrap()
            .iter()
            .filter(|e| e.2 == "workspace.restored")
            .count(),
        2
    );
    assert!(app.edits.files.iter().all(|f| !f.changed()));
}
#[test]
fn manual_conflict_blocks_all_revert_but_not_an_unrelated_selected_file() {
    let (root, store, session, mut app) = setup();
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("original a"),
        "model a",
    );
    edit(
        root.path(),
        &store,
        &session,
        "b.txt",
        Some("original b"),
        "model b",
    );
    fs::write(root.path().join("b.txt"), "manual b after model").unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    key(&mut app, root.path(), &store, KeyCode::Char('U'));
    assert!(app.edits.pending.is_none());
    assert!(app.diagnostic.as_ref().unwrap().contains("b.txt"));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "model a"
    );
    key(&mut app, root.path(), &store, KeyCode::Esc);
    key(&mut app, root.path(), &store, KeyCode::Char('u'));
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "original a"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("b.txt")).unwrap(),
        "manual b after model"
    );
}
#[test]
fn stale_preview_and_live_execution_never_apply_a_revert() {
    let (root, store, session, mut app) = setup();
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("original"),
        "model",
    );
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    key(&mut app, root.path(), &store, KeyCode::Char('u'));
    store
        .event(&session, None, "user.note", "history changed")
        .unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert!(app
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("changed since preview"));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "model"
    );
    key(&mut app, root.path(), &store, KeyCode::Esc);
    let owner = store.claim_execution(&session.id, None).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('u'));
    assert!(app.edits.pending.is_none());
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "model"
    );
    store.finish_execution(&session.id, &owner, None).unwrap();
}
#[test]
fn edits_show_missing_snapshot_errors_instead_of_claiming_no_changes() {
    let (root, store, session, mut app) = setup();
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("original"),
        "model",
    );
    let blob: String = store
        .db
        .query_row(
            "SELECT after_blob FROM file_changes WHERE session=?1",
            [&session.id],
            |r| r.get(0),
        )
        .unwrap();
    fs::remove_file(store.directory.join("blobs").join(blob)).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    assert!(app.edits.error.is_some());
    assert!(app.edits.files.is_empty());
}

#[test]
fn manual_edits_after_preview_are_not_overwritten_and_other_branches_are_out_of_scope() {
    let (root, store, mut session, mut app) = setup();
    let start = store.history(&session).unwrap()[0].0;
    edit(
        root.path(),
        &store,
        &session,
        "parent.txt",
        Some("parent original"),
        "parent branch edit",
    );
    store.fork(&mut session, start, "evaluate").unwrap();
    edit(
        root.path(),
        &store,
        &session,
        "child.txt",
        Some("child original"),
        "child edit",
    );
    app.refresh(&store).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    assert_eq!(app.edits.files.len(), 1);
    assert_eq!(app.edits.files[0].path, "child.txt");
    key(&mut app, root.path(), &store, KeyCode::Char('U'));
    fs::write(root.path().join("child.txt"), "new manual edits").unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert!(app
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("changed since preview"));
    assert_eq!(
        fs::read_to_string(root.path().join("child.txt")).unwrap(),
        "new manual edits"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("parent.txt")).unwrap(),
        "parent branch edit"
    );
}

#[test]
fn edits_tab_renders_at_supported_sizes_and_detail_navigation_keeps_file_selection() {
    use ratatui::{backend::TestBackend, Terminal};
    let (root, store, session, mut app) = setup();
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("old a\n"),
        "new a\n",
    );
    edit(root.path(), &store, &session, "b.txt", None, "new b\n");
    key(&mut app, root.path(), &store, KeyCode::Char('3'));
    for (width, height) in [(40, 14), (60, 20), (80, 24), (130, 40)] {
        app.viewport = (width, height);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("a.txt"));
    }
    app.viewport = (80, 24);
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Detail);
    key(&mut app, root.path(), &store, KeyCode::Down);
    assert_eq!(app.edits.index, 0);
    key(&mut app, root.path(), &store, KeyCode::Enter);
    key(&mut app, root.path(), &store, KeyCode::Down);
    assert_eq!(app.edits.index, 1);
    assert!(app.export_snapshot().unwrap().text.contains("new b"));
}

#[test]
fn restore_engine_rechecks_history_under_ownership_before_applying() {
    let (root, store, session, _app) = setup();
    let start = store.history(&session).unwrap()[0].0;
    edit(
        root.path(),
        &store,
        &session,
        "a.txt",
        Some("original"),
        "candidate",
    );
    let plan = RestorePlan::prepare_paths(
        root.path(),
        &store,
        &session,
        start,
        Some(&["a.txt".into()]),
    )
    .unwrap();
    store
        .event(&session, None, "user.note", "arrived after preview")
        .unwrap();
    let error = plan
        .apply_silent(root.path(), &store, &session)
        .unwrap_err();
    assert!(error.to_string().contains("history changed since preview"));
    assert_eq!(
        fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "candidate"
    );
}
