use super::*;
use crate::manager::{Decision, Reply};
use ratatui::{backend::TestBackend, Terminal};
#[test]
fn explicit_idle_route_controls_preserve_help_priority_and_show_queue_withdrawal() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("workflows/demo");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        root.path().join("greenlight.yaml"),
        "provider:\n  model: fixture\nmanager:\n  enabled: true\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: a\nsteps: [a, b]\n",
    )
    .unwrap();
    for step in ["a", "b"] {
        std::fs::write(
            folder.join(format!("{step}.md")),
            format!("---\nid: {step}\nverification:\n  type: none\n---\nWork\n"),
        )
        .unwrap();
    }
    let store = Store::open(root.path()).unwrap();
    let session = store.create("demo", "goal", "a").unwrap();
    let saved =
        crate::manager::save_message(&store, &session.id, "TO_B keep this text exactly").unwrap();
    let snapshot =
        crate::manager::capture(root.path(), &store, &session.id, &saved.message_id, &[]).unwrap();
    store
        .event(
            &session,
            None,
            "manager.snapshot",
            &serde_json::to_string(&snapshot).unwrap(),
        )
        .unwrap();
    let reply = Reply {
        decision: Decision::Forward {
            schema_version: 1,
            message_id: saved.message_id.clone(),
            snapshot_id: snapshot.snapshot_id,
            content: "Destination b".into(),
            citations: vec!["workflow:demo".into()],
            step_id: "b".into(),
        },
        status: "forward_proposed".into(),
        snapshot_changed: false,
        captured_at: snapshot.captured_at,
        sources: snapshot.sources,
    };
    let event = store
        .event(
            &session,
            None,
            "manager.result",
            &serde_json::to_string(&reply).unwrap(),
        )
        .unwrap();
    let mut app = App::new(Some(&session.id));
    app.refresh(&store).unwrap();
    app.event_index = app.events.iter().position(|e| e.id == event).unwrap();
    app.follow_activity = false;
    assert!(app.footer_actions().contains(&shell::Action::DeliverRoute));
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal.draw(|f| draw(f, &app)).unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(rendered.contains("Deliver original"));
    app.open_help();
    app.on_key(
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    assert!(store.manager_routes(&session).unwrap().is_empty());
    app.on_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        root.path(),
        &store,
    )
    .unwrap();
    app.activate(shell::Action::DeliverRoute, root.path(), &store)
        .unwrap();
    assert!(app.runner.is_none());
    assert_eq!(store.manager_routes(&session).unwrap()[0].state, "queued");
    app.refresh(&store).unwrap();
    app.event_index = app
        .events
        .iter()
        .rposition(|e| e.kind == "manager.delivery")
        .unwrap();
    assert!(app.footer_actions().contains(&shell::Action::WithdrawRoute));
    app.activate(shell::Action::WithdrawRoute, root.path(), &store)
        .unwrap();
    assert_eq!(
        store.manager_routes(&session).unwrap()[0].state,
        "withdrawn"
    );
    app.activate(shell::Action::RouteStatus, root.path(), &store)
        .unwrap();
    assert!(app.context_detail.as_ref().unwrap().contains("withdrawn"));
    assert!(app.runner.is_none());
    assert!(store
        .usage_report(&session, crate::usage::Scope::Session)
        .unwrap()
        .calls
        .is_empty());
}
