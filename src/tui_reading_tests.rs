//! Reading/export behavior through real keyboard routing and immutable IDs.
use super::*;
fn key(app: &mut App, root: &Path, store: &Store, key: KeyCode) {
    assert!(!app
        .on_key(KeyEvent::new(key, KeyModifiers::NONE), root, store)
        .unwrap());
}
fn fixture() -> (tempfile::TempDir, Store, crate::store::Session, i64) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("demo", "Goal", "work").unwrap();
    let event = store
        .event(
            &session,
            Some("work"),
            "user.instruction",
            &"Long evidence\n".repeat(40),
        )
        .unwrap();
    store
        .add_artifact(&session, "work", "report.md", &"Long artifact\n".repeat(40))
        .unwrap();
    (root, store, session, event)
}
#[test]
fn each_tab_keeps_its_selection_focus_scroll_and_draft_across_polling() {
    let (root, store, s, event) = fixture();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.event_index = app.events.iter().position(|e| e.id == event).unwrap();
    app.follow_activity = false;
    app.focus = Focus::Detail;
    app.detail_scroll = 7;
    app.composer = "Unsent instructions".into();
    key(&mut app, root.path(), &store, KeyCode::Char('2'));
    assert_eq!(app.detail_scroll, 0);
    key(&mut app, root.path(), &store, KeyCode::Right);
    for _ in 0..4 {
        key(&mut app, root.path(), &store, KeyCode::Down);
    }
    key(&mut app, root.path(), &store, KeyCode::Char('1'));
    assert_eq!(app.event().unwrap().id, event);
    assert_eq!(app.focus, Focus::Detail);
    assert_eq!(app.detail_scroll, 7);
    assert!(!app.follow_activity);
    store
        .event(&s, None, "user.instruction", "new activity")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id, event);
    assert_eq!(app.detail_scroll, 7);
    key(&mut app, root.path(), &store, KeyCode::Char('2'));
    assert_eq!(app.detail_scroll, 4);
    assert_eq!(app.focus, Focus::Detail);
    assert_eq!(app.composer, "Unsent instructions");
}
#[test]
fn session_and_branch_scopes_restore_their_own_positions() {
    let (root, store, mut a, _) = fixture();
    let old_branch = a.active_branch.clone();
    let cutoff = store.artifacts(&a, "session").unwrap()[0].event_id;
    let b = store.create("demo", "Other", "work").unwrap();
    store
        .add_artifact(&b, "work", "other.md", "Other body")
        .unwrap();
    let mut app = App::new(Some(&a.id));
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    app.focus = Focus::Detail;
    app.detail_scroll = 5;
    app.requested_session = Some(b.id);
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 0);
    app.focus = Focus::Detail;
    app.detail_scroll = 2;
    app.requested_session = Some(a.id.clone());
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 5);
    store.fork(&mut a, cutoff, "work").unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 0);
    app.focus = Focus::Detail;
    app.detail_scroll = 3;
    store.switch_branch(&mut a, &old_branch, "work").unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 5);
    key(&mut app, root.path(), &store, KeyCode::Char('1'));
}
#[test]
fn help_and_context_scroll_do_not_overwrite_the_tab_position() {
    let (root, store, s, _) = fixture();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.focus = Focus::Detail;
    app.follow_activity = false;
    app.detail_scroll = 8;
    key(&mut app, root.path(), &store, KeyCode::Char('?'));
    key(&mut app, root.path(), &store, KeyCode::Down);
    key(&mut app, root.path(), &store, KeyCode::Down);
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert_eq!(app.detail_scroll, 8);
    assert_eq!(app.focus, Focus::Detail);
    key(&mut app, root.path(), &store, KeyCode::Char('c'));
    key(&mut app, root.path(), &store, KeyCode::Down);
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert_eq!(app.detail_scroll, 8);
}
#[test]
fn a_replacement_artifact_resets_scroll_instead_of_reusing_an_old_body_offset() {
    let (root, store, s, _) = fixture();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    app.focus = Focus::Detail;
    app.detail_scroll = 9;
    key(&mut app, root.path(), &store, KeyCode::Char('?'));
    store
        .add_artifact(&s, "work", "report.md", "replacement")
        .unwrap();
    app.refresh(&store).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert_eq!(app.detail_scroll, 0);
    assert_eq!(app.artifact().unwrap().content, "replacement");
}
#[test]
fn activity_filter_is_unicode_aware_and_scoped_and_export_keys_are_literal_in_it() {
    let (root, store, s, _) = fixture();
    let request = store
        .event(
            &s,
            Some("work"),
            "tool.request",
            "action: tool\ntool: read\npath: résumé.md\n",
        )
        .unwrap();
    store
        .tool_outcome(&s, "work", request, "read", "tool.result", "body")
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('F'));
    for ch in "RÉSUMÉ".chars() {
        key(&mut app, root.path(), &store, KeyCode::Char(ch));
    }
    assert_eq!(app.events.len(), 1);
    assert_eq!(app.event().unwrap().kind, "tool.result");
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert_eq!(app.activity_query, "RÉSUMÉ");
    assert!(!app.activity_filtering);
    let other = store.create("demo", "Other session", "work").unwrap();
    app.requested_session = Some(other.id);
    app.refresh(&store).unwrap();
    assert!(app.activity_query.is_empty());
    app.requested_session = Some(s.id.clone());
    app.refresh(&store).unwrap();
    assert_eq!(app.activity_query, "RÉSUMÉ");
    assert_eq!(app.events.len(), 1);
    key(&mut app, root.path(), &store, KeyCode::Char('F'));
    key(&mut app, root.path(), &store, KeyCode::Char('C'));
    key(&mut app, root.path(), &store, KeyCode::Char('E'));
    assert!(app.events.is_empty());
    assert!(app.copy_job.is_none());
    assert!(app.pending_editor.is_none());
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert!(app.activity_query.is_empty());
    assert!(app.events.len() > 1);
}
#[test]
fn export_snapshot_is_full_source_and_editor_refuses_an_active_worker() {
    use std::sync::mpsc;
    let (root, store, s, _) = fixture();
    let source = format!("# Title\n{}\u{001b}[2J", "large original\n".repeat(10_000));
    store
        .add_artifact(&s, "work", "report.md", &source)
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    assert_eq!(app.export_snapshot().unwrap().text, source);
    let count = store.history(&s).unwrap().len();
    key(&mut app, root.path(), &store, KeyCode::Char('E'));
    assert_eq!(app.pending_editor.take().unwrap().text, source);
    assert_eq!(store.history(&s).unwrap().len(), count);
    let (_pt, prompts) = mpsc::channel();
    let (answers, _rx) = mpsc::channel();
    let (_ft, finished) = mpsc::channel();
    app.runner = Some(Runner {
        cancel_retry: None,
        session_id: Some(s.id),
        prompts,
        answers,
        finished,
    });
    key(&mut app, root.path(), &store, KeyCode::Char('E'));
    assert!(app.pending_editor.is_none());
    assert!(app.notice.contains("approvals stay available"));
    app.approval = Some(ApprovalPrompt {
        content: "Explicit decision".into(),
        mode: ApprovalMode::Human,
    });
    key(&mut app, root.path(), &store, KeyCode::Char('C'));
    key(&mut app, root.path(), &store, KeyCode::Char('E'));
    assert!(app.copy_job.is_none());
    assert!(app.pending_editor.is_none());
    assert!(app.approval.is_some());
}

#[test]
fn journal_diff_reading_state_survives_tabs_and_session_hops() {
    let (root, store, s, _) = fixture();
    let id = store
        .add_change(&s, "work", "file.rs", Some("before\n"), "after\n")
        .unwrap();
    let other = store.create("demo", "Other", "work").unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.event_index = app.events.iter().position(|e| e.id == id).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('d'));
    app.detail_scroll = 2;
    key(&mut app, root.path(), &store, KeyCode::Char('2'));
    key(&mut app, root.path(), &store, KeyCode::Char('1'));
    assert!(app.detail().contains("-before"));
    assert_eq!(app.detail_scroll, 2);
    app.requested_session = Some(other.id);
    app.refresh(&store).unwrap();
    assert!(app.diff.is_none());
    app.requested_session = Some(s.id);
    app.refresh(&store).unwrap();
    assert!(app.detail().contains("+after"));
    assert_eq!(app.detail_scroll, 2);
}
