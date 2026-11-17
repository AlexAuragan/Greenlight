//! Controller regression tests.
use super::*;

#[test]
fn disconnected_worker_expires_approval_and_does_not_replay_it() {
    let (prompt_tx, prompts) = std::sync::mpsc::channel();
    let (answers, old_answers) = std::sync::mpsc::channel();
    let (finished_tx, finished) = std::sync::mpsc::channel();
    let mut app = App::new(None);
    app.runner = Some(Runner { live:None,
        cancel_retry: None,
        prompts,
        answers,
        finished,
        session_id: None,
    });
    app.approval = Some(ApprovalPrompt { command: None,
        content: "old request".into(),
        mode: ApprovalMode::Persistent,
    });
    drop(prompt_tx);
    drop(finished_tx); // A worker panic disconnects without a completion message.
    app.tick();
    assert!(app.runner.is_none());
    assert!(app.approval.is_none());
    assert!(app.notice.contains("pending approval expired"));
    assert!(old_answers.try_recv().is_err());
    let (_prompt_tx, prompts) = std::sync::mpsc::channel();
    let (answers, fresh_answers) = std::sync::mpsc::channel();
    let (_finished_tx, finished) = std::sync::mpsc::channel();
    app.runner = Some(Runner { live:None,
        cancel_retry: None,
        prompts,
        answers,
        finished,
        session_id: None,
    });
    assert!(app.deliver_approval('y').is_err());
    assert!(fresh_answers.try_recv().is_err());
}

#[test]
fn enter_toggles_evidence_and_i_opens_editable_composer() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("example", "goal", "plan").unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    assert_eq!(
        app.focus,
        Focus::Events,
        "opening a session must not auto-focus the composer"
    );
    app.on_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Detail);
    app.on_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Events);
    app.on_key(
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        root.path(),
        &store,
    ).unwrap();
    assert_eq!(app.focus, Focus::Compose);
    app.on_key(
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    app.on_key(
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    app.on_key(
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.composer, "x");
    app.on_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Events);
}

#[test]
fn focus_moves_across_panes_and_arrows_scroll_detail() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("example", "goal", "plan").unwrap();
    store
        .add_artifact(&session, "plan", "plan.md", "# Heading\n\nLong artifact")
        .unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    assert_eq!(app.focus, Focus::Events);
    assert_eq!(app.focus, Focus::Events);
    app.on_key(
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Artifacts);
    app.on_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Detail);
    let selected = app.artifact_index;
    app.on_key(
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    app.on_key(
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.detail_scroll, 2);
    app.on_key(
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(
        app.detail_scroll, 10,
        "PageDown adds a page without changing selection"
    );
    assert_eq!(app.artifact_index, selected);
    app.on_key(
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.detail_scroll, 9);
    app.on_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Artifacts);
    app.on_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Sessions);
    app.on_key(
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Compose);
    app.on_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.focus, Focus::Sessions);
}

#[test]
fn restart_confirmation_restores_and_creates_fork() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut session = store.create("example", "goal", "plan").unwrap();
    let checkpoint = store
        .event(&session, Some("plan"), "step.completed", "ok")
        .unwrap();
    store
        .event(&session, Some("implement"), "step.started", "model")
        .unwrap();
    store
        .add_change(
            &session,
            "implement",
            "file.txt",
            Some("before\n"),
            "after\n",
        )
        .unwrap();
    std::fs::write(root.path().join("file.txt"), "after\n").unwrap();
    session.status = "blocked".into();
    store.save(&session).unwrap();
    let previous_branch = session.active_branch.clone();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.restart_choices = vec![RestartCandidate {
        step: "implement".into(),
        prior_event: checkpoint,
    }];
    app.prepare_restart(root.path(), &store);
    assert!(app.restart_plan.is_some(), "{}", app.notice);
    app.confirm_restart(root.path(), &store);
    assert_eq!(
        std::fs::read_to_string(root.path().join("file.txt")).unwrap(),
        "before\n"
    );
    let fork = store.session(&session.id).unwrap();
    assert_ne!(fork.active_branch, previous_branch);
    assert_eq!(fork.current_step, "implement");
    assert_eq!(fork.status, "ready");
    assert!(app.restart_choices.is_empty());
    assert_eq!(app.focus, Focus::Compose);
}

#[test]
fn approval_shortcuts_submit_immediately_but_enter_does_not() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let (prompts_tx, prompts_rx) = std::sync::mpsc::channel();
    let (answer_tx, answer_rx) = std::sync::mpsc::channel();
    let (_finish_tx, finish_rx) = std::sync::mpsc::channel();
    let mut app = App::new(None);
    app.runner = Some(Runner { live:None,
        cancel_retry: None,
        session_id: None,
        prompts: prompts_rx,
        answers: answer_tx,
        finished: finish_rx,
    });
    app.approval = Some(ApprovalPrompt { command: None,
        content: "Approve a shell command".into(),
        mode: ApprovalMode::Persistent,
    });
    app.on_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(answer_rx.try_recv().is_err());
    assert!(app.approval.is_some());
    app.on_key(
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.detail_scroll, 1);
    app.on_key(
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.detail_scroll, 9);
    app.on_key(
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(answer_rx.try_recv().unwrap(), "g");
    assert!(app.approval.is_none());
    drop(prompts_tx);
}

#[test]
fn human_approval_rejects_global_scope_and_escape_denies() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let (_prompts_tx, prompts_rx) = std::sync::mpsc::channel();
    let (answer_tx, answer_rx) = std::sync::mpsc::channel();
    let (_finish_tx, finish_rx) = std::sync::mpsc::channel();
    let mut app = App::new(None);
    app.runner = Some(Runner { live:None,
        cancel_retry: None,
        session_id: None,
        prompts: prompts_rx,
        answers: answer_tx,
        finished: finish_rx,
    });
    app.approval = Some(ApprovalPrompt { command: None,
        content: "Human checkpoint".into(),
        mode: ApprovalMode::Human,
    });
    app.on_key(
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(answer_rx.try_recv().is_err());
    assert!(app.approval.is_some());
    app.on_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(answer_rx.try_recv().unwrap(), "n");
    app.approval = Some(ApprovalPrompt { command: None,
        content: "Another human checkpoint".into(),
        mode: ApprovalMode::Human,
    });
    app.on_key(
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(answer_rx.try_recv().unwrap(), "y");
}

#[test]
fn artifact_tab_displays_latest_branch_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let session = store.create("example", "goal", "plan").unwrap();
    store
        .add_artifact(&session, "plan", "plan.md", "first version")
        .unwrap();
    store
        .add_artifact(&session, "plan", "plan.md", "revised version")
        .unwrap();
    store
        .add_artifact(&session, "plan", "notes.md", "Notes")
        .unwrap();

    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.on_key(
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.view, View::Artifacts);
    assert_eq!(app.focus, Focus::Artifacts);
    assert_eq!(app.artifacts.len(), 2);
    let plan_index = app
        .artifacts
        .iter()
        .position(|a| a.name == "plan.md")
        .unwrap();
    app.artifact_index = plan_index;
    assert_eq!(app.artifact().unwrap().content, "revised version");
    store
        .add_artifact(&session, "plan", "plan.md", "third version")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.artifact().unwrap().name, "plan.md");
    assert_eq!(app.artifact().unwrap().content, "third version");
    assert!(app.render_markdown);
    app.detail_scroll = 10;
    app.on_key(
        KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(!app.render_markdown);
    assert_eq!(app.detail_scroll, 0);
    app.on_key(
        KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(app.render_markdown);
    let backend = ratatui::backend::TestBackend::new(110, 32);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    app.on_key(
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert_eq!(app.view, View::Conversation);
    assert_eq!(app.focus, Focus::Events);
}

#[test]
fn artifact_tab_handles_no_saved_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    store.create("example", "goal", "plan").unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    app.switch_view(View::Artifacts);
    let backend = ratatui::backend::TestBackend::new(100, 26);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    assert!(app.artifact().is_none());
}

#[test]
fn navigation_clamps_at_boundaries() {
    assert_eq!(move_index(0, -1, 0), 0);
    assert_eq!(move_index(0, -1, 5), 0);
    assert_eq!(move_index(3, 1, 4), 3);
    assert_eq!(move_index(2, -1, 4), 1);
}

#[test]
fn concise_history_filters_protocol_chatter() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    let session = store.create("fix-bug", "test", "explore").unwrap();
    store
        .event(&session, Some("explore"), "model.output", "raw response")
        .unwrap();
    store
        .event(&session, Some("explore"), "protocol.error", "bad action")
        .unwrap();
    store
        .event(
            &session,
            Some("explore"),
            "permission.decision",
            "write: approved once",
        )
        .unwrap();
    store
        .event(&session, Some("explore"), "tool.error", "user denied write")
        .unwrap();
    store
        .event(&session, Some("explore"), "human.approved", "")
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    assert!(app.events.iter().any(|e| e.kind == "protocol.error"));
    assert!(app.events.iter().all(|e| e.kind != "model.output"));
    assert!(app.events.iter().all(|e| e.kind != "permission.decision"));
    assert!(app.events.iter().any(|e| e.kind == "tool.error"));
    assert!(app.events.iter().any(|e| e.kind == "human.approved"));
    app.toggle_verbose();
    app.refresh(&store).unwrap();
    assert!(app.events.iter().any(|e| e.kind == "model.output"));
    assert!(app.events.iter().any(|e| e.kind == "permission.decision"));
}

#[test]
fn session_refresh_preserves_selected_event() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    let session = store.create("fix-bug", "test", "explore").unwrap();
    store
        .event(&session, Some("explore"), "step.started", "start")
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.focus = Focus::Events;
    app.navigate(-1);
    let selected = app.event().unwrap().id;
    store
        .event(&session, Some("explore"), "step.completed", "done")
        .unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(app.event().unwrap().id, selected);
}

#[test]
fn renders_dashboard_with_test_backend() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    store.create("add-feature", "test", "explore").unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
}

#[test]
fn diff_reads_original_journal_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    let session = store.create("add-feature", "test", "implement").unwrap();
    store
        .add_change(&session, "implement", "file.txt", Some("old\n"), "new\n")
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.toggle_diff(&store).unwrap();
    let detail = app.detail();
    assert!(detail.contains("-old"));
    assert!(detail.contains("+new"));
    app.toggle_diff(&store).unwrap();
    assert!(!app.detail().contains("--- a/file.txt"));
}
