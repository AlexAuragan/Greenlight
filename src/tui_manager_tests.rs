//! Real local HTTP worker handoff, without paid inference or real approval effects.
use super::*;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
};

fn key(app: &mut App, root: &Path, store: &Store, code: KeyCode) {
    assert!(!app
        .on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
        .unwrap());
}
#[test]
fn manager_job_runs_beside_owner_preserves_text_and_cannot_answer_approvals() {
    let root = tempfile::tempdir().unwrap();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let env_name = format!("GREENLIGHT_MANAGER_TEST_{}", uuid::Uuid::new_v4().simple());
    std::env::set_var(&env_name, "fixture-key");
    let folder = root.path().join("workflows/demo");
    fs::create_dir_all(&folder).unwrap();
    fs::write(root.path().join("greenlight.yaml"),format!("provider:\n  model: fixture\n  base_url: http://{address}/v1\n  api_key_env: {env_name}\nmanager:\n  enabled: true\n")).unwrap();
    fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
    )
    .unwrap();
    fs::write(
        folder.join("work.md"),
        "---\nid: work\nverification:\n  type: none\n---\nWork\n",
    )
    .unwrap();
    fs::write(
        root.path().join("README.md"),
        "# Project\nTODO: investigate\n",
    )
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    let s = store.create("demo", "Goal", "work").unwrap();
    let owner = store.claim_execution(&s.id, None).unwrap();
    let (arrived, request) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 1024];
        let body = loop {
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break serde_json::from_slice::<serde_json::Value>(
                        &bytes[end + 4..end + 4 + length],
                    )
                    .unwrap();
                }
            }
        };
        let input: serde_json::Value =
            serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        arrived.send(input.clone()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let decision = serde_json::json!({"schema_version":1,"message_id":input["message_id"],"snapshot_id":input["evidence"]["snapshot_id"],"action":"answer","content":"The project TODO says investigate; the active pipeline has not recorded completion.","citations":["file:README.md"]});
        let body=serde_json::json!({"choices":[{"message":{"content":decision.to_string()}}],"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":40},"completion_tokens":30}}).to_string();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let mut app = App::new(Some(&s.id));
    app.refresh(&store).unwrap();
    assert!(app.manager_enabled);
    let (_prompts_tx, prompts) = mpsc::channel();
    let (answers, answers_rx) = mpsc::channel();
    let (_finished_tx, finished) = mpsc::channel();
    app.runner = Some(Runner { live:None,
        prompts,
        finished,
        answers,
        session_id: Some(s.id.clone()),
        cancel_retry: None,
    });
    let original = "  Where are we on TODO? 漢字\n preserve trailing spaces  \n";
    app.composer = original.into();
    app.focus = Focus::Compose;
    app.submit(root.path(), &store);
    assert!(app.manager_job.is_some());
    assert!(app.composer.is_empty());
    let input = request.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(input["text"], original);
    app.approval = Some(ApprovalPrompt { review: None, command: None,
        content: "Synthetic coding approval".into(),
        mode: ApprovalMode::Tool,
    });
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(answers_rx.try_recv().is_err());
    key(&mut app, root.path(), &store, KeyCode::Char('y'));
    assert_eq!(
        answers_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "y"
    );
    app.runner = None;
    key(&mut app, root.path(), &store, KeyCode::Char('q'));
    assert!(app.manager_job.is_some());
    app.cancel_manager();
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.manager_job.is_some() {
        app.tick();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    worker.join().unwrap();
    app.refresh(&store).unwrap();
    assert_eq!(store.session(&s.id).unwrap().status, "running");
    let report = store
        .usage_report(&s, crate::usage::Scope::Session)
        .unwrap();
    assert_eq!(report.totals.requests, 1);
    assert_eq!(report.totals.cached_input.reported, 40);
    assert_eq!(report.calls[0].purpose, "manager");
    app.composer = "/manager".into();
    app.submit(root.path(), &store);
    assert!(app.events.iter().all(|e| e.kind.starts_with("manager.")));
    app.event_index = app
        .events
        .iter()
        .position(|e| e.kind == "manager.result")
        .unwrap();
    key(&mut app, root.path(), &store, KeyCode::Enter);
    assert!(app.detail().contains("file:README.md"));
    assert!(app.detail().contains("TODO says investigate"));
    for (width, height) in [(100, 24), (40, 14)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        app.viewport = (width, height);
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }
    let count = store.history(&s).unwrap().len();
    app.composer = "/manager-context".into();
    app.submit(root.path(), &store);
    assert!(app
        .context_detail
        .as_ref()
        .unwrap()
        .contains("no inference"));
    assert_eq!(count, store.history(&s).unwrap().len());
    store.finish_execution(&s.id, &owner, None).unwrap();
    std::env::remove_var(env_name);
}
