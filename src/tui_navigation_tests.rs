//! UI behavior under background events, session changes and terminal resizing.
use super::*;
use ratatui::{backend::TestBackend, Terminal};
use std::sync::mpsc;

fn key(app: &mut App, store: &Store, root: &Path, code: KeyCode) {
    assert!(!app
        .on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
        .unwrap());
}
fn screen(app: &mut App, width: u16, height: u16) -> String {
    app.viewport = (width, height);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
#[test]
fn drafts_follow_session_ids_through_filtering_and_reordering() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let a = store.create("demo", "Alpha", "work").unwrap();
    let b = store.create("demo", "Beta", "work").unwrap();
    let mut app = App::new(Some(&a.id));
    app.refresh(&store).unwrap();
    app.composer = "Alpha instructions\n第二行".into();
    app.requested_session = Some(b.id.clone());
    app.refresh(&store).unwrap();
    assert!(app.composer.is_empty());
    app.composer = "Beta draft".into();
    store
        .event(&a, None, "user.message", "background activity")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.composer, "Beta draft");
    app.requested_session = Some(a.id.clone());
    app.refresh(&store).unwrap();
    assert_eq!(app.composer, "Alpha instructions\n第二行");
    app.session_query = "no matches".into();
    app.refresh(&store).unwrap();
    assert!(app.session().is_none());
    assert!(app.composer.is_empty());
    app.session_query.clear();
    app.requested_session = Some(a.id);
    app.refresh(&store).unwrap();
    assert_eq!(app.composer, "Alpha instructions\n第二行");
    app.requested_session = Some(b.id);
    app.refresh(&store).unwrap();
    assert_eq!(app.composer, "Beta draft");
}
#[test]
fn paused_evidence_survives_polling_and_new_activity_until_explicit_follow() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let first = store
        .event(&s, None, "user.message", &"evidence\n".repeat(30))
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    key(&mut app, &store, root.path(), KeyCode::Right);
    assert_eq!(app.focus, Focus::Detail);
    assert!(!app.follow_activity);
    app.detail_scroll = 4;
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 4);
    let newest = store
        .event(&s, None, "user.message", "new activity")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id, first);
    assert_eq!(app.detail_scroll, 4);
    key(&mut app, &store, root.path(), KeyCode::Left);
    key(&mut app, &store, root.path(), KeyCode::End);
    assert!(app.follow_activity);
    assert_eq!(app.event().unwrap().id, newest);
    let last = store
        .event(&s, None, "user.message", "follow this")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id, last);
    key(&mut app, &store, root.path(), KeyCode::Up);
    assert!(!app.follow_activity);
    app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id, newest);
}
#[test]
fn artifact_scroll_survives_background_events() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    store
        .add_artifact(&s, "work", "report.md", &"long report\n".repeat(40))
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    app.focus = Focus::Detail;
    app.detail_scroll = 8;
    store
        .event(&s, None, "user.message", "new activity")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.detail_scroll, 8);
    assert!(screen(&mut app, 80, 24).contains("long report"));
}
#[test]
fn help_is_scrollable_and_does_not_consume_or_execute_the_message_draft() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.composer = "Unsent instructions".into();
    key(&mut app, &store, root.path(), KeyCode::Char('?'));
    assert!(screen(&mut app, 80, 24).contains("Keyboard help"));
    key(&mut app, &store, root.path(), KeyCode::End);
    assert!(screen(&mut app, 60, 20).contains("quit when"));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert_eq!(app.composer, "Unsent instructions");
    assert!(app.runner.is_none());
    app.focus_composer();
    key(&mut app, &store, root.path(), KeyCode::Char('?'));
    assert_eq!(app.composer, "Unsent instructions?");
    assert!(!app.help);
    app.composer = "/help".into();
    key(&mut app, &store, root.path(), KeyCode::Enter);
    assert!(app.help);
    assert!(app.runner.is_none());
}
#[test]
fn resize_clamps_scroll_and_preserves_draft_selection_and_reading_position() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.focus = Focus::Detail;
    app.composer = "draft survives resize".into();
    app.context_detail = Some(format!(
        "{}\nTAIL",
        "line of readable evidence\n".repeat(50)
    ));
    app.detail_scroll = u16::MAX;
    for (w, h) in [(80, 24), (60, 20), (40, 14), (120, 40), (80, 24)] {
        let output = screen(&mut app, w, h);
        assert!(output.contains("TAIL"), "tail missing at {w}x{h}");
        assert_eq!(app.session().unwrap().id, s.id);
        assert_eq!(app.composer, "draft survives resize");
    }
    let (_, clamped) = app.rendered_scroll.get().unwrap();
    assert!(clamped > 0);
    key(&mut app, &store, root.path(), KeyCode::Up);
    assert_eq!(app.detail_scroll, clamped - 1);
    screen(&mut app, 12, 6);
    key(&mut app, &store, root.path(), KeyCode::Char('i'));
    assert_eq!(app.composer, "draft survives resize");
    assert!(screen(&mut app, 80, 24).contains("draft survives resize"));
}
#[test]
fn tiny_approval_cannot_be_accepted_and_resize_keeps_its_request() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    let (_pt, prompts) = mpsc::channel();
    let (answers, received) = mpsc::channel();
    let (_ft, finished) = mpsc::channel();
    app.runner = Some(Runner {
        session_id: None,
        prompts,
        answers,
        finished,
    });
    app.approval = Some(ApprovalPrompt {
        content: "Sensitive global scope".into(),
        mode: ApprovalMode::ConfirmPersistent,
    });
    assert!(screen(&mut app, 40, 14).contains("Decisions disabled"));
    key(&mut app, &store, root.path(), KeyCode::Char('y'));
    assert!(received.try_recv().is_err());
    assert!(app.approval.is_some());
    assert!(screen(&mut app, 60, 20).contains("Sensitive global scope"));
    key(&mut app, &store, root.path(), KeyCode::Char('?'));
    assert!(!app.help);
    assert!(received.try_recv().is_err());
    key(&mut app, &store, root.path(), KeyCode::Enter);
    assert!(received.try_recv().is_err());
    key(&mut app, &store, root.path(), KeyCode::Char('y'));
    assert_eq!(received.try_recv().unwrap(), "y");
}
#[test]
fn long_composer_keeps_its_cursor_end_visible() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.focus_composer();
    app.composer = format!("{}END OF DRAFT", "a long message line\n".repeat(20));
    assert!(screen(&mut app, 60, 20).contains("END OF DRAFT"));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert!(app.composer.ends_with("END OF DRAFT"));
}

#[test]
fn approval_owner_switch_preserves_both_session_drafts() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let owner = store.create("demo", "Owner", "work").unwrap();
    let other = store.create("demo", "Other", "work").unwrap();
    let mut app = App::new(Some(&owner.id));
    app.refresh(&store).unwrap();
    app.composer = "Owner draft".into();
    app.requested_session = Some(other.id.clone());
    app.refresh(&store).unwrap();
    app.composer = "Other draft".into();
    let (pt, prompts) = mpsc::channel();
    let (answers, _rx) = mpsc::channel();
    let (_ft, finished) = mpsc::channel();
    app.runner = Some(Runner {
        session_id: Some(owner.id.clone()),
        prompts,
        answers,
        finished,
    });
    pt.send(ApprovalPrompt {
        content: "Review owner request".into(),
        mode: ApprovalMode::Human,
    })
    .unwrap();
    app.tick();
    app.refresh(&store).unwrap();
    assert_eq!(app.session().unwrap().id, owner.id);
    assert_eq!(app.composer, "Owner draft");
    key(&mut app, &store, root.path(), KeyCode::Esc);
    app.requested_session = Some(other.id);
    app.refresh(&store).unwrap();
    assert_eq!(app.composer, "Other draft");
}
#[test]
fn compact_layout_shows_picker_then_activity_and_handles_zero_size() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Readable goal", "work").unwrap();
    store
        .event(&s, None, "user.message", "Activity evidence")
        .unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    assert!(screen(&mut app, 60, 20).contains("Sessions"));
    key(&mut app, &store, root.path(), KeyCode::Right);
    assert!(screen(&mut app, 60, 20).contains("Activity evidence"));
    for (w, h) in [(0, 0), (1, 1), (30, 10)] {
        screen(&mut app, w, h);
    }
    screen(&mut app, 60, 20);
    key(&mut app, &store, root.path(), KeyCode::Left);
    assert_eq!(app.focus, Focus::Sessions);
    assert!(screen(&mut app, 60, 20).contains("Readable goal"));
}
