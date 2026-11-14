//! Picker interaction tests avoid provider calls and exercise the 80×24 surface.
use super::*;
use ratatui::{backend::TestBackend, Terminal};
fn key(app: &mut App, root: &Path, store: &Store, code: KeyCode) {
    assert!(!app
        .on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
        .unwrap());
}
fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn workflow(root: &Path) {
    let folder = root.join("workflows/demo");
    std::fs::create_dir_all(&folder).unwrap();
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
}
#[test]
fn search_handles_unicode_no_matches_clear_and_selection_stability() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let a = store.create("demo", "Écrire parser", "work").unwrap();
    let b = store.create("demo", "Other work", "work").unwrap();
    let mut app = App::new(Some(&a.id));
    app.refresh(&store).unwrap();
    store
        .event(&b, None, "user.message", "new activity")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.session().unwrap().id, a.id);
    key(&mut app, root.path(), &store, KeyCode::Char('f'));
    for c in "ÉCRIRE".chars() {
        key(&mut app, root.path(), &store, KeyCode::Char(c));
    }
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.session().unwrap().id, a.id);
    assert!(screen(&app).contains("Find session"));
    key(&mut app, root.path(), &store, KeyCode::Char('x'));
    assert!(app.sessions.is_empty());
    assert!(screen(&app).contains("0 of 2 sessions"));
    key(&mut app, root.path(), &store, KeyCode::Backspace);
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(!app.searching);
    assert_eq!(app.session_query, "ÉCRIRE");
    key(&mut app, root.path(), &store, KeyCode::Char('f'));
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert!(app.session_query.is_empty());
    assert_eq!(app.sessions.len(), 2);
}
#[test]
fn new_session_form_creates_ready_session_without_execution_or_draft_loss() {
    let root = tempfile::tempdir().unwrap();
    workflow(root.path());
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    app.composer = "Saved instructions".into();
    key(&mut app, root.path(), &store, KeyCode::Char('n'));
    assert!(screen(&app).contains("choose workflow"));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(store.list().unwrap().is_empty());
    for c in "Repair café".chars() {
        key(&mut app, root.path(), &store, KeyCode::Char(c));
    }
    assert!(screen(&app).contains("Repair café"));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(app.new_session.is_none());
    assert!(app.runner.is_none());
    assert_eq!(app.session().unwrap().goal, "Repair café");
    assert_eq!(app.session().unwrap().status, "ready");
    assert!(app.composer.is_empty());
    assert_eq!(app.drafts.get(&None).map(String::as_str), Some("Saved instructions"));
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(store.attempts(app.session().unwrap()).unwrap().is_empty());
    key(&mut app, root.path(), &store, KeyCode::Char('n'));
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert_eq!(store.list().unwrap().len(), 1);
}
#[test]
fn new_session_form_explains_empty_workflow_directory() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('n'));
    assert!(screen(&app).contains("No workflows found"));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn pending_approval_clears_filter_that_hides_its_owner() {
    use std::sync::mpsc;
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let owner = store.create("demo", "Owner goal", "work").unwrap();
    let other = store.create("demo", "Other goal", "work").unwrap();
    let mut app = App::new(Some(&other.id));
    app.session_query = "Other".into();
    app.refresh(&store).unwrap();
    assert_eq!(app.sessions.len(), 1);
    let (sender, prompts) = mpsc::channel();
    let (answers, _received) = mpsc::channel();
    let (_finished, finished) = mpsc::channel();
    app.runner = Some(Runner {
        session_id: Some(owner.id.clone()),
        prompts,
        answers,
        finished,
    });
    app.searching = true;
    sender
        .send(ApprovalPrompt {
            content: "Owner permission".into(),
            mode: ApprovalMode::Human,
        })
        .unwrap();
    app.tick();
    assert!(app.session_query.is_empty());
    assert!(!app.searching);
    assert!(app.requested_session.is_some());
    app.refresh(&store).unwrap();
    assert_eq!(app.session().unwrap().id, owner.id);
    assert!(screen(&app).contains("Owner goal"));
    assert!(screen(&app).contains("Owner permission"));
}
