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
    assert_eq!(app.navigation.candidates.len(), 1);
    assert_eq!(app.session().unwrap().id, a.id);
    assert!(screen(&app).contains("Find session"));
    key(&mut app, root.path(), &store, KeyCode::Char('x'));
    assert!(app.navigation.candidates.is_empty());
    assert!(screen(&app).contains("0 of 2 sessions"));
    key(&mut app, root.path(), &store, KeyCode::Backspace);
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(!app.searching);
    assert_eq!(app.session_query, "ÉCRIRE");
    key(&mut app, root.path(), &store, KeyCode::Char('f'));
    app.activate(shell::Action::ClearSearch,root.path(),&store).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Esc);
    assert!(app.session_query.is_empty());
    assert_eq!(app.navigation.candidates.len(), 2);
}
#[test]
fn new_session_form_starts_worker_without_draft_loss_and_keeps_approvals() {
    let root = tempfile::tempdir().unwrap();
    workflow(root.path());
    std::fs::write(
        root.path().join("greenlight.yaml"),
        "provider:\n  model: fixture\n  base_url: http://127.0.0.1:1\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("workflows/demo/work.md"),
        "---\nid: work\nkind: human\nverification:\n  type: none\n---\nReview before execution\n",
    )
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    app.composer = "Saved instructions".into();
    key(&mut app, root.path(), &store, KeyCode::Char('n'));
    assert!(screen(&app).contains("Choose workflow"));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(store.list().unwrap().is_empty());
    for c in "Repair café".chars() {
        key(&mut app, root.path(), &store, KeyCode::Char(c));
    }
    assert!(screen(&app).contains("Repair café"));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(app.new_session.is_none());
    assert!(app.runner.is_some());
    assert!(screen(&app).contains("starting"));
    assert_eq!(app.session().unwrap().goal, "Repair café");
    assert_eq!(app.session().unwrap().status, "ready");
    assert!(app.composer.is_empty());
    assert_eq!(
        app.drafts.get(&(None,None,compose::Mode::Step)).map(String::as_str),
        Some("Saved instructions")
    );
    assert_eq!(store.list().unwrap().len(), 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.approval.is_none() {
        assert!(Instant::now() < deadline, "{}", app.notice);
        app.tick();
        app.refresh(&store).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(screen(&app).contains("Review before execution"));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(app.approval.is_some());
    assert!(app.checkpoint.reading);
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert!(app.approval.is_some());
    key(&mut app, root.path(), &store, KeyCode::Esc);
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    while app.runner.is_some() {
        assert!(Instant::now() < deadline, "{}", app.notice);
        app.tick();
        app.refresh(&store).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.session().unwrap().status, "completed");
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
    assert_eq!(app.navigation.candidates.len(), 1);
    let (sender, prompts) = mpsc::channel();
    let (answers, _received) = mpsc::channel();
    let (_finished, finished) = mpsc::channel();
    app.runner = Some(Runner {
        live: None,
        cancel_retry: None,
        session_id: Some(owner.id.clone()),
        prompts,
        answers,
        finished,
    });
    app.open_navigator(true);
    sender
        .send(ApprovalPrompt { review: None, command: None,
            content: "Owner permission".into(),
            mode: ApprovalMode::Human,
        })
        .unwrap();
    app.tick();
    assert!(app.session_query.is_empty());
    assert!(!app.searching);
    assert!(!app.shell.sessions);
    assert_eq!(app.focus,app.main_focus());
    assert!(app.requested_session.is_some());
    app.refresh(&store).unwrap();
    assert_eq!(app.session().unwrap().id, owner.id);
    assert!(screen(&app).contains("Owner goal"));
    assert!(screen(&app).contains("Owner permission"));
}

#[test]
fn new_goal_starts_real_http_stream_before_action_execution_and_records_usage() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (release, gate) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    let body: serde_json::Value =
                        serde_json::from_slice(&request[end + 4..end + 4 + length]).unwrap();
                    assert_eq!(body["stream"], true);
                    assert_eq!(body["stream_options"]["include_usage"], true);
                    break;
                }
            }
        }
        let event = |text: &str| {
            format!(
                "data: {}\n\n",
                serde_json::json!({"id":"local-stream","model":"fixture","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})
            )
        };
        let first = event("action: finish\n");
        let last=format!("{}data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":20,\"completion_tokens\":3,\"prompt_tokens_details\":{{\"cached_tokens\":5}}}}}}\n\ndata: [DONE]\n\n",event("summary: Completed local fixture"));
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{first}",first.len()+last.len()).unwrap();
        socket.flush().unwrap();
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        socket.write_all(last.as_bytes()).unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    workflow(root.path());
    let env = format!("GREENLIGHT_STREAM_TEST_{}", uuid::Uuid::new_v4().simple());
    std::env::set_var(&env, "local-fixture");
    std::fs::write(
        root.path().join("greenlight.yaml"),
        format!(
            "provider:\n  model: fixture\n  base_url: http://{address}/v1\n  api_key_env: {env}\n"
        ),
    )
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    let mut app = App::new(None);
    app.refresh(&store).unwrap();
    key(&mut app, root.path(), &store, KeyCode::Char('n'));
    key(&mut app, root.path(), &store, KeyCode::Enter);
    for c in "Local streamed goal".chars() {
        key(&mut app, root.path(), &store, KeyCode::Char(c));
    }
    key(&mut app, root.path(), &store, KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.tick();
        app.refresh(&store).unwrap();
        let text = app
            .runner
            .as_ref()
            .and_then(|r| r.live.as_ref())
            .map(|p| p.lock().unwrap().text.clone())
            .unwrap_or_default();
        if text.contains("action: finish") {
            break;
        }
        assert!(Instant::now() < deadline, "{}", app.notice);
        std::thread::sleep(Duration::from_millis(5));
    }
    let rendered = screen(&app);
    assert!(rendered.contains("action: finish"));
    assert!(rendered.contains("running"));
    let history = store.history(app.session().unwrap()).unwrap();
    assert!(!history.iter().any(|(_, _, kind, _)| matches!(
        kind.as_str(),
        "model.output" | "step.completed" | "tool.request"
    )));
    release.send(()).unwrap();
    while app.runner.is_some() {
        assert!(Instant::now() < deadline, "{}", app.notice);
        app.tick();
        app.refresh(&store).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.session().unwrap().status, "completed");
    let report = store
        .usage_report(app.session().unwrap(), crate::usage::Scope::Session)
        .unwrap();
    assert_eq!(report.calls.len(), 1);
    assert_eq!(report.calls[0].counts.input.value, Some(20));
    assert_eq!(report.calls[0].counts.cached_input.value, Some(5));
    assert_eq!(report.calls[0].counts.output.value, Some(3));
    server.join().unwrap();
    std::env::remove_var(env);
}

#[test]
fn candidate_browsing_filtering_and_cancel_preserve_open_workspace_and_drafts() {
    let root=tempfile::tempdir().unwrap();let store=Store::open(root.path()).unwrap();
    let a=store.create("demo","Alpha goal","work").unwrap();let b=store.create("demo","Beta goal","work").unwrap();
    store.event(&a,None,"user.message","Alpha recorded evidence").unwrap();
    let mut app=App::new(Some(&a.id));app.refresh(&store).unwrap();app.composer="Alpha draft".into();
    let event=app.event().unwrap().id;let before=store.history(&a).unwrap().len();
    app.open_navigator(false);app.navigation.selected=Some(b.id.clone());app.navigation.refresh(&app.sessions,&app.session_query,&app.session_activity);
    assert_eq!(app.session().unwrap().id,a.id);assert_eq!(app.composer,"Alpha draft");assert_eq!(app.event().unwrap().id,event);
    store.event(&a,None,"user.message","Background reorder").unwrap();app.refresh(&store).unwrap();
    assert_eq!(app.navigation.candidate().unwrap().id,b.id);assert_eq!(app.session().unwrap().id,a.id);
    app.searching=true;for c in "missing".chars(){key(&mut app,root.path(),&store,KeyCode::Char(c));}
    assert!(app.navigation.candidates.is_empty());assert_eq!(app.session().unwrap().id,a.id);assert_eq!(app.composer,"Alpha draft");
    key(&mut app,root.path(),&store,KeyCode::Esc);assert!(!app.shell.sessions);assert_eq!(app.session().unwrap().id,a.id);
    app.session_query.clear();app.open_navigator(false);app.navigation.selected=Some(b.id.clone());app.navigation.refresh(&app.sessions,&app.session_query,&app.session_activity);
    app.open_candidate(&store).unwrap();assert_eq!(app.session().unwrap().id,b.id);assert!(app.composer.is_empty());
    app.composer="Beta draft".into();app.open_navigator(false);app.navigation.selected=Some(a.id.clone());app.navigation.refresh(&app.sessions,&app.session_query,&app.session_activity);app.open_candidate(&store).unwrap();
    assert_eq!(app.composer,"Alpha draft");assert_eq!(store.history(&a).unwrap().len(),before+1);
    assert!(app.runner.is_none());assert!(app.approval.is_none());
}
#[test]
fn missing_active_and_candidate_sessions_never_substitute_a_different_run() {
    let root=tempfile::tempdir().unwrap();let store=Store::open(root.path()).unwrap();
    let a=store.create("demo","Alpha","work").unwrap();let b=store.create("demo","Beta","work").unwrap();
    let mut app=App::new(Some(&a.id));app.refresh(&store).unwrap();app.open_navigator(false);
    app.navigation.selected=Some(b.id.clone());app.navigation.refresh(&app.sessions,&app.session_query,&app.session_activity);
    store.db.execute("DELETE FROM events WHERE session=?1",[&b.id]).unwrap();store.db.execute("DELETE FROM branches WHERE session=?1",[&b.id]).unwrap();store.db.execute("DELETE FROM sessions WHERE id=?1",[&b.id]).unwrap();
    app.refresh(&store).unwrap();assert!(app.navigation.candidate().is_none());
    app.open_candidate(&store).unwrap();assert_eq!(app.session().unwrap().id,a.id);
    app.close_navigator();
    store.db.execute("DELETE FROM events WHERE session=?1",[&a.id]).unwrap();store.db.execute("DELETE FROM branches WHERE session=?1",[&a.id]).unwrap();store.db.execute("DELETE FROM sessions WHERE id=?1",[&a.id]).unwrap();
    let c=store.create("demo","Third run","work").unwrap();app.refresh(&store).unwrap();
    assert!(app.session().is_none());assert!(app.navigation.missing.is_some());assert_eq!(app.navigation.opened.as_deref(),Some(a.id.as_str()));
    assert!(screen(&app).contains("Opened session unavailable"));
    app.open_navigator(false);app.navigation.select(1);app.open_candidate(&store).unwrap();assert_eq!(app.session().unwrap().id,c.id);
}
#[test]
fn discovery_and_history_errors_are_distinct_from_empty_and_retry_is_read_only() {
    let root=tempfile::tempdir().unwrap();crate::project::init(root.path()).unwrap();let store=Store::open(root.path()).unwrap();
    let s=store.create("fix-bug","Existing run","investigate").unwrap();let mut app=App::new(Some(&s.id));app.refresh(&store).unwrap();
    store.db.execute("ALTER TABLE sessions RENAME TO hidden_sessions",[]).unwrap();app.refresh(&store).unwrap();
    assert!(app.navigation.error.is_some());assert!(screen(&app).contains("Session discovery unavailable"));assert!(!screen(&app).contains("No sessions in this project"));
    store.db.execute("ALTER TABLE hidden_sessions RENAME TO sessions",[]).unwrap();
    key(&mut app,root.path(),&store,KeyCode::Tab);assert_eq!(app.shell_actions()[app.shell.action_index],shell::Action::RetryDiscovery);
    key(&mut app,root.path(),&store,KeyCode::Enter);
    assert_eq!(app.session().unwrap().id,s.id);assert!(app.navigation.error.is_none());
    store.db.execute("UPDATE branches SET parent='missing-parent' WHERE id=?1",[&s.active_branch]).unwrap();app.refresh(&store).unwrap();
    assert!(app.navigation.load_error.is_some());assert!(screen(&app).contains("Opened session data unavailable"));
    store.db.execute("UPDATE branches SET parent=NULL WHERE id=?1",[&s.active_branch]).unwrap();app.refresh(&store).unwrap();
    assert!(app.navigation.load_error.is_none());assert_eq!(store.history(&s).unwrap().len(),1);assert!(app.runner.is_none());
}

#[test]
fn mouse_candidate_selection_is_not_open_and_stale_row_release_is_ignored() {
    use crossterm::event::{MouseEvent,MouseEventKind,MouseButton};
    let root=tempfile::tempdir().unwrap();let store=Store::open(root.path()).unwrap();
    let a=store.create("demo","Alpha session","work").unwrap();let b=store.create("demo","Beta session","work").unwrap();
    let mut app=App::new(Some(&a.id));app.refresh(&store).unwrap();app.open_navigator(false);app.viewport=(120,36);
    let mut terminal=Terminal::new(TestBackend::new(120,36)).unwrap();terminal.draw(|f|view::draw(f,&app)).unwrap();
    let row=terminal.backend().buffer().content.chunks(120).position(|row|row.iter().map(|c|c.symbol()).collect::<String>().contains("Beta session")).unwrap() as u16;
    let mouse=|app:&mut App,kind| {app.on_event(Event::Mouse(MouseEvent{kind,column:4,row,modifiers:KeyModifiers::NONE}),root.path(),&store).unwrap();};
    mouse(&mut app,MouseEventKind::Down(MouseButton::Left));
    store.event(&a,None,"user.message","Reorder the candidates").unwrap();app.refresh(&store).unwrap();terminal.draw(|f|view::draw(f,&app)).unwrap();
    mouse(&mut app,MouseEventKind::Up(MouseButton::Left));
    assert_eq!(app.session().unwrap().id,a.id);assert_eq!(app.navigation.candidate().unwrap().id,a.id);
    // Fresh click on the current Beta row selects it, but still leaves Alpha open.
    let row=terminal.backend().buffer().content.chunks(120).position(|row|row.iter().map(|c|c.symbol()).collect::<String>().contains("Beta session")).unwrap() as u16;
    for kind in [MouseEventKind::Down(MouseButton::Left),MouseEventKind::Up(MouseButton::Left)] {
        app.on_event(Event::Mouse(MouseEvent{kind,column:4,row,modifiers:KeyModifiers::NONE}),root.path(),&store).unwrap();
    }
    assert_eq!(app.session().unwrap().id,a.id);assert_eq!(app.navigation.candidate().unwrap().id,b.id);
    app.activate(shell::Action::OpenCandidate,root.path(),&store).unwrap();assert_eq!(app.session().unwrap().id,b.id);
    app.open_navigator(true);app.on_event(Event::Paste("hjn /resume".into()),root.path(),&store).unwrap();
    assert_eq!(app.session_query,"hjn /resume");assert_eq!(app.session().unwrap().id,b.id);assert!(app.runner.is_none());
    app.close_navigator();assert!(app.session_query.contains("/resume"));
}
