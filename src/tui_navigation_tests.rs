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
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        terminal.draw(|f| draw(f, app)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        if !output
            .split_whitespace()
            .collect::<String>()
            .contains("Preparingdocumentpreview")
        {
            return output;
        }
        assert!(Instant::now() < deadline, "preview worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn enter_returns_from_readers_without_composing_sending_or_losing_selection() {
    let root=tempfile::tempdir().unwrap();let store=Store::open(root.path()).unwrap();
    let s=store.create("demo","Goal","work").unwrap();store.add_artifact(&s,"work","report.md","# Report\nbody\n").unwrap();
    let count=store.history(&s).unwrap().len();
    for size in [(100,28),(40,14)] {
        let mut app=App::new(Some(&s.id));app.refresh(&store).unwrap();app.viewport=size;app.composer="unsent draft".into();
        for (view,list) in [(View::Conversation,Focus::Events),(View::Artifacts,Focus::Artifacts)] {
            app.switch_view(view);app.focus=list;
            let event=app.event().map(|e|e.id);let artifact=app.artifact().map(|a|a.event_id);
            key(&mut app,&store,root.path(),KeyCode::Enter);assert_eq!(app.focus,Focus::Detail);
            if view==View::Conversation {assert!(screen(&mut app,size.0,size.1).contains("Enter returns"));}
            app.on_key(KeyEvent::new_with_kind(KeyCode::Enter,KeyModifiers::NONE,KeyEventKind::Repeat),root.path(),&store).unwrap();assert_eq!(app.focus,Focus::Detail);
            key(&mut app,&store,root.path(),KeyCode::Enter);assert_eq!(app.focus,list);
            assert_eq!(app.event().map(|e|e.id),event);assert_eq!(app.artifact().map(|a|a.event_id),artifact);assert_eq!(app.composer,"unsent draft");assert!(app.runner.is_none());assert!(app.manager_job.is_none());
            key(&mut app,&store,root.path(),KeyCode::Enter);key(&mut app,&store,root.path(),KeyCode::Char('i'));assert_eq!(app.focus,Focus::Compose);
            key(&mut app,&store,root.path(),KeyCode::Esc);assert_eq!(app.focus,Focus::Detail);
            key(&mut app,&store,root.path(),KeyCode::Left);assert_eq!(app.focus,list);
        }
        app.view=View::Artifacts;app.focus=Focus::Artifacts;key(&mut app,&store,root.path(),KeyCode::Char('v'));
        assert!(!app.artifact_browser.as_ref().unwrap().reading);
        key(&mut app,&store,root.path(),KeyCode::Enter);assert!(app.artifact_browser.as_ref().unwrap().reading);
        key(&mut app,&store,root.path(),KeyCode::Enter);assert!(!app.artifact_browser.as_ref().unwrap().reading);
        assert_eq!(app.composer,"unsent draft");assert_eq!(count,store.history(&s).unwrap().len());
    }
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
        cancel_retry: None,
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
        cancel_retry: None,
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

#[test]
fn artifact_modes_escape_controls_and_keep_the_original_stored_source() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("demo", "Goal", "work").unwrap();
    let original = "# Unsafe \x1b[2J\n\u{009b}31m\t漢字";
    store
        .add_artifact(&session, "work", "report.md", original)
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    app.focus = Focus::Detail;
    for markdown in [true, false] {
        app.render_markdown = markdown;
        let output = screen(&mut app, 120, 40);
        assert!(output.contains("\\u{001b}"));
        assert!(output.contains("\\u{009b}"));
        assert!(!output.contains('\x1b'));
        assert!(!output.contains('\u{009b}'));
    }
    assert_eq!(
        store.artifacts(&session, "session").unwrap()[0].content,
        original
    );
    let large = "large line ".repeat(10_000);
    store
        .add_artifact(&session, "work", "report.md", &large)
        .unwrap();
    app.refresh(&store).unwrap();
    for markdown in [true, false] {
        app.render_markdown = markdown;
        app.detail_scroll = u16::MAX;
        assert!(screen(&mut app, 120, 40).contains("Preview truncated"));
    }
    assert_eq!(
        store.artifacts(&session, "session").unwrap()[0].content,
        large
    );
}

#[test]
fn artifact_history_navigation_provenance_diff_and_refresh_are_read_only() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    store
        .add_artifact(&s, "work", "report.md", "old line\n")
        .unwrap();
    let old = store.artifacts(&s, "session").unwrap()[0].event_id;
    store
        .add_artifact(&s, "work", "report.md", "new line\n")
        .unwrap();
    let current = store.artifacts(&s, "session").unwrap()[0].event_id;
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.composer = "Keep this draft".into();
    app.switch_view(View::Artifacts);
    key(&mut app, &store, root.path(), KeyCode::Char('v'));
    assert!(screen(&mut app, 80, 24).contains("Versions"));
    assert_eq!(
        app.artifact_browser.as_ref().unwrap().versions[0].event_id,
        current
    );
    key(&mut app, &store, root.path(), KeyCode::Down);
    assert_eq!(
        app.artifact_browser.as_ref().unwrap().versions[1].event_id,
        old
    );
    assert_eq!(app.artifact_browser.as_ref().unwrap().text, "old line\n");
    key(&mut app, &store, root.path(), KeyCode::Char('p'));
    assert!(screen(&mut app, 80, 24).contains("Creation event:"));
    assert!(app
        .artifact_browser
        .as_ref()
        .unwrap()
        .text
        .contains(&format!("#{old}")));
    key(&mut app, &store, root.path(), KeyCode::Left);
    key(&mut app, &store, root.path(), KeyCode::Up);
    key(&mut app, &store, root.path(), KeyCode::Char('d'));
    assert!(screen(&mut app, 80, 24).contains("-old line"));
    assert!(app
        .artifact_browser
        .as_ref()
        .unwrap()
        .text
        .contains("+new line"));
    store
        .add_artifact(&s, "work", "report.md", "latest line\n")
        .unwrap();
    let history_count = store.history(&s).unwrap().len();
    app.refresh(&store).unwrap();
    let browser = app.artifact_browser.as_ref().unwrap();
    assert_eq!(browser.versions[browser.index].event_id, current);
    assert!(browser.text.contains("+new line"));
    assert!(!browser.text.contains("latest line"));
    screen(&mut app, 60, 20);
    key(&mut app, &store, root.path(), KeyCode::Char('o'));
    assert_eq!(app.artifact_browser.as_ref().unwrap().text, "new line\n");
    key(&mut app, &store, root.path(), KeyCode::Char('m'));
    assert!(screen(&mut app, 60, 20).contains("new line"));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert!(app.artifact_browser.is_none());
    assert_eq!(app.composer, "Keep this draft");
    assert_eq!(app.artifact().unwrap().content, "latest line\n");
    assert_eq!(store.history(&s).unwrap().len(), history_count);
}
#[test]
fn artifact_history_closes_after_branch_switch_and_approvals_take_priority() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut s = store.create("demo", "Goal", "work").unwrap();
    store
        .add_artifact(&s, "work", "report.md", "ancestor")
        .unwrap();
    let cutoff = store.artifacts(&s, "session").unwrap()[0].event_id;
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    key(&mut app, &store, root.path(), KeyCode::Char('v'));
    store.fork(&mut s, cutoff, "work").unwrap();
    app.refresh(&store).unwrap();
    assert!(app.artifact_browser.is_none());
    key(&mut app, &store, root.path(), KeyCode::Char('v'));
    let (pt, prompts) = mpsc::channel();
    let (answers, _rx) = mpsc::channel();
    let (_ft, finished) = mpsc::channel();
    app.runner = Some(Runner {
        cancel_retry: None,
        session_id: Some(s.id),
        prompts,
        answers,
        finished,
    });
    pt.send(ApprovalPrompt {
        content: "Review request".into(),
        mode: ApprovalMode::Human,
    })
    .unwrap();
    app.tick();
    assert!(app.artifact_browser.is_none());
    assert!(app.approval.is_some());
    key(&mut app, &store, root.path(), KeyCode::Char('v'));
    assert!(app.artifact_browser.is_none());
}

#[test]
fn retry_cancellation_is_explicit_and_does_not_interfere_with_text_or_approvals() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let (_pt, prompts) = mpsc::channel();
    let (answers, _rx) = mpsc::channel();
    let (_ft, finished) = mpsc::channel();
    app.runner = Some(Runner {
        cancel_retry: Some(cancel.clone()),
        session_id: None,
        prompts,
        answers,
        finished,
    });
    app.focus_composer();
    key(&mut app, &store, root.path(), KeyCode::Char('x'));
    assert_eq!(app.composer, "x");
    assert!(!cancel.load(Ordering::SeqCst));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    app.approval = Some(ApprovalPrompt {
        content: "Approval stays explicit".into(),
        mode: ApprovalMode::Human,
    });
    key(&mut app, &store, root.path(), KeyCode::Char('x'));
    assert!(!cancel.load(Ordering::SeqCst));
    assert!(app.approval.is_some());
    app.approval = None;
    key(&mut app, &store, root.path(), KeyCode::Char('x'));
    assert!(cancel.load(Ordering::SeqCst));
    assert!(app.notice.contains("in-flight request may finish"));
}

#[test]
fn activity_cards_expand_exact_tool_request_and_output_and_keep_raw_audit_mode() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let request = store
        .event(
            &s,
            Some("work"),
            "tool.request",
            "action: tool\ntool: read\npath: notes.md\n",
        )
        .unwrap();
    let output = store
        .tool_outcome(
            &s,
            "work",
            request,
            "read",
            "tool.result",
            "file body\nall tests passed",
        )
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    assert_eq!(app.events.len(), 2);
    assert_eq!(app.event().unwrap().id, output);
    assert!(screen(&mut app, 80, 24).contains("Tool · read · returned"));
    key(&mut app, &store, root.path(), KeyCode::Enter);
    let detail = app.detail();
    assert!(detail.contains(&format!("Recorded request #{request}")));
    assert!(detail.contains(&format!("Recorded output #{output}")));
    assert!(detail.contains("path: notes.md"));
    assert!(detail.contains("file body"));
    store
        .event(&s, Some("work"), "artifact.created", "later.md")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id, output);
    assert!(!app.follow_activity);
    key(&mut app, &store, root.path(), KeyCode::Left);
    key(&mut app, &store, root.path(), KeyCode::Char('v'));
    app.refresh(&store).unwrap();
    assert!(app.events.iter().any(|e| e.kind == "tool.finished"));
    assert!(app.events.iter().any(|e| e.id == request));
    assert!(!app.detail().contains("Recorded request"));
    assert!(app.detail().contains("file body"));
    screen(&mut app, 40, 14);
}
#[test]
fn artifact_card_opens_its_exact_version_and_file_card_uses_journal_diff() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    store
        .add_artifact(&s, "work", "report.md", "older body")
        .unwrap();
    let old = store.artifacts(&s, "session").unwrap()[0].event_id;
    store
        .add_artifact(&s, "work", "report.md", "latest body")
        .unwrap();
    let change = store
        .add_change(&s, "work", "file.rs", Some("before\n"), "after\n")
        .unwrap();
    let count = store.history(&s).unwrap().len();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    app.event_index = app.events.iter().position(|e| e.id == old).unwrap();
    app.follow_activity = false;
    key(&mut app, &store, root.path(), KeyCode::Char('o'));
    assert_eq!(app.artifact_browser.as_ref().unwrap().text, "older body");
    key(&mut app, &store, root.path(), KeyCode::Esc);
    app.event_index = app.events.iter().position(|e| e.id == change).unwrap();
    key(&mut app, &store, root.path(), KeyCode::Char('d'));
    assert!(app.detail().contains("-before"));
    assert!(app.detail().contains("+after"));
    assert_eq!(app.focus, Focus::Detail);
    assert!(screen(&mut app, 80, 24).contains("-before"));
    assert_eq!(store.history(&s).unwrap().len(), count);
}
#[test]
fn invalid_card_correlations_remain_inspectable_in_concise_history() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let id = store
        .event(
            &s,
            Some("work"),
            "tool.finished",
            r#"{"request_event":9999,"outcome_event":9998,"tool":"read","exit_code":null}"#,
        )
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    assert!(app.diagnostic.as_ref().unwrap().contains("branch-visible"));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert_eq!(app.event().unwrap().id, id);
    assert_eq!(app.event().unwrap().kind, "event.invalid");
    assert!(screen(&mut app, 80, 24).contains("Malformed record"));
    assert!(app.detail().contains("9999"));
}

#[test]
fn model_intent_cards_are_labelled_searchable_and_expand_original_response() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let raw = "action: tool\ntool: write\npath: source.rs\ncontent: full source evidence\nactivity_title: Exploring call sites";
    let source = store.event(&s, Some("work"), "model.output", raw).unwrap();
    let title = store
        .event(
            &s,
            Some("work"),
            "activity.title",
            &serde_json::json!({
                "model_event": source, "title": "Exploring call sites"
            })
            .to_string(),
        )
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    assert!(!app.events.iter().any(|e| e.id == source));
    assert_eq!(app.event().unwrap().id, title);
    assert!(screen(&mut app, 100, 24).contains("Agent intent · Exploring call sites"));
    key(&mut app, &store, root.path(), KeyCode::Enter);
    let detail = app.detail();
    assert!(detail.contains("Model-authored intent only"));
    assert!(detail.contains(&format!("Recorded model response #{source}")));
    assert!(detail.contains(raw));
    let snapshot = app.export_snapshot().unwrap();
    assert!(snapshot.text.contains(raw));
    screen(&mut app, 80, 24);
    screen(&mut app, 40, 14);
    key(&mut app, &store, root.path(), KeyCode::Left);
    key(&mut app, &store, root.path(), KeyCode::Char('F'));
    for ch in "EXPLORING".chars() {
        key(&mut app, &store, root.path(), KeyCode::Char(ch));
    }
    key(&mut app, &store, root.path(), KeyCode::Enter);
    assert_eq!(app.events.len(), 1);
    assert_eq!(app.event().unwrap().id, title);
    key(&mut app, &store, root.path(), KeyCode::Char('F'));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    key(&mut app, &store, root.path(), KeyCode::Char('v'));
    app.refresh(&store).unwrap();
    assert!(app.events.iter().any(|e| e.id == source));
    app.event_index = app.events.iter().position(|e| e.id == title).unwrap();
    assert!(app.detail().contains("\"model_event\""));
    assert!(!app.detail().contains("Recorded model response"));
}

#[test]
fn malformed_intent_annotation_stays_visible_as_diagnostic() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let id = store
        .event(
            &s,
            Some("work"),
            "activity.title",
            r#"{"model_event":9999,"title":"Verified success"}"#,
        )
        .unwrap();
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    assert!(app.diagnostic.as_ref().unwrap().contains("branch-visible"));
    key(&mut app, &store, root.path(), KeyCode::Esc);
    assert_eq!(app.event().unwrap().id, id);
    assert_eq!(app.event().unwrap().kind, "event.invalid");
    assert!(screen(&mut app, 80, 24).contains("Malformed record"));
    assert!(app.detail().contains("9999"));
}

#[test]
fn workspace_audit_card_expands_classification_and_scan_metrics_without_mutation() {
    let root=tempfile::tempdir().unwrap();let store=Store::open(root.path()).unwrap();
    let s=store.create("demo","Goal","work").unwrap();
    let before=crate::workspace::record(root.path(),&store,&s,"work","before").unwrap();
    std::fs::write(root.path().join("manual.txt"),"external body").unwrap();
    let after=crate::workspace::record(root.path(),&store,&s,"work","after").unwrap();
    let audit=crate::workspace_audit::correlate(&store,&s,before.0,after.0,Some(after.0),&before.1,&after.1).unwrap();
    let id=store.event(&s,Some("work"),"workspace.correlation",&serde_json::to_string(&audit).unwrap()).unwrap();
    let count=store.history(&s).unwrap().len();
    let mut app=App::new(Some(&s.id));app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id,id);
    assert!(screen(&mut app,100,24).contains("Workspace audit · 0 explained · 1 unexplained"));
    key(&mut app,&store,root.path(),KeyCode::Enter);
    let detail=app.detail();assert!(detail.contains("manual.txt"));assert!(detail.contains("Actor is unknown"));assert!(detail.contains("bytes hashed"));
    assert!(!detail.contains("external body"));screen(&mut app,40,14);
    assert_eq!(count,store.history(&s).unwrap().len());
    assert_eq!(std::fs::read_to_string(root.path().join("manual.txt")).unwrap(),"external body");
}
