use super::*;
use crate::{engine::Engine, provider::Script};
use std::{
    fs,
    sync::{Arc, Mutex},
};
struct Fixture {
    root: tempfile::TempDir,
    engine: Engine,
    session: Session,
    script: Arc<Mutex<Script>>,
}
fn respond(messages: &[Value]) -> std::result::Result<String, String> {
    if let Ok(value) = serde_json::from_str::<Value>(messages[1]["content"].as_str().unwrap_or(""))
    {
        if let Some(evidence) = value.get("evidence") {
            let target = if value["text"].as_str().unwrap_or("").contains("TO_B") {
                "b"
            } else {
                "a"
            };
            return Ok(json!({"schema_version":1,"message_id":value["message_id"],"snapshot_id":evidence["snapshot_id"],"action":"forward","step_id":target,"content":"This explanation must never replace the original.","citations":["workflow:demo"]}).to_string());
        }
    }
    Ok("action: finish\nsummary: Done".into())
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: fixture\nmanager:\n  enabled: true\n",
        )
        .unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: a\nsteps: [a, b, c]\n",
        )
        .unwrap();
        for (step, next) in [("a", Some("b")), ("b", Some("c")), ("c", None)] {
            fs::write(
                folder.join(format!("{step}.md")),
                format!(
                    "---\nid: {step}\nverification:\n  type: none\n{}---\nSTEP_{step}\n",
                    next.map(|id| format!("transitions:\n  success: {id}\n"))
                        .unwrap_or_default()
                ),
            )
            .unwrap();
        }
        let mut engine = Engine::open(root.path()).unwrap();
        let script = Arc::new(Mutex::new(Script {
            responder: Some(respond),
            ..Default::default()
        }));
        engine.provider.script = Some(script.clone());
        let session = engine.store.create("demo", "goal", "a").unwrap();
        Self {
            root,
            engine,
            session,
            script,
        }
    }
    async fn proposal(&self, text: &str) -> i64 {
        self.engine
            .ask_manager(&self.session.id, text, &[])
            .await
            .unwrap();
        self.engine
            .store
            .history(&self.session)
            .unwrap()
            .iter()
            .rev()
            .find(|e| e.2 == "manager.result")
            .unwrap()
            .0
    }
    fn accept(&self, event: i64) -> (Route, bool, bool) {
        let accepted = self
            .engine
            .store
            .accept_manager_route(self.root.path(), &self.session.id, event)
            .unwrap();
        (
            accepted.route,
            accepted.newly_accepted,
            accepted.start_current,
        )
    }
    fn routes(&self) -> Vec<Route> {
        self.engine
            .store
            .manager_routes(&self.engine.store.session(&self.session.id).unwrap())
            .unwrap()
    }
    fn coding(&self) -> Vec<Vec<Value>> {
        self.script
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|messages| messages[0]["content"] != crate::manager::system_prompt())
            .cloned()
            .collect()
    }
}
#[tokio::test]
async fn future_step_receives_exact_original_once_without_run_wide_leakage() {
    let mut f = Fixture::new();
    let original = " \tTO_B — optimize é with threads\r\nKeep every byte.  ";
    let event = f.proposal(original).await;
    assert!(f.coding().is_empty());
    let (route, created, start) = f.accept(event);
    assert!(created && !start);
    assert_eq!(route.original, original);
    assert!(!f.accept(event).1);
    f.engine.run(&f.session.id, None, None).await.unwrap();
    let calls = f.coding();
    assert_eq!(calls.len(), 3);
    for (index, messages) in calls.iter().enumerate() {
        let occurrences = messages
            .iter()
            .filter(|m| m["role"] == "user" && m["content"].as_str() == Some(original))
            .count();
        assert_eq!(occurrences, usize::from(index == 1));
        assert!(!messages
            .iter()
            .any(|m| m["content"].as_str()
                == Some("This explanation must never replace the original.")));
    }
    let delivered = &f.routes()[0];
    assert_eq!(delivered.state, "included_in_request");
    assert!(delivered.attempt.is_some() && delivered.call.is_some());
    assert!(f
        .engine
        .store
        .withdraw_manager_route(&f.session.id, &route.message_id)
        .is_err());
    assert!(f
        .engine
        .run_manager_route(&f.session.id, &route.message_id)
        .await
        .is_err());
    assert_eq!(f.coding().len(), 3);
}
#[tokio::test]
async fn identical_text_is_distinct_fifo_guidance_and_repeated_accept_never_duplicates() {
    let mut f = Fixture::new();
    let text = "TO_A same instruction";
    let first = f.proposal(text).await;
    let second = f.proposal(text).await;
    let r2 = f.accept(second).0;
    let r1 = f.accept(first).0;
    assert_ne!(r1.message_id, r2.message_id);
    assert!(!f.accept(first).1);
    f.engine
        .run_manager_route(&f.session.id, &r2.message_id)
        .await
        .unwrap();
    let messages = &f.coding()[0];
    assert_eq!(
        messages
            .iter()
            .filter(|m| m["content"].as_str() == Some(text))
            .count(),
        2
    );
    let metadata = messages
        .iter()
        .filter_map(|m| serde_json::from_str::<Value>(m["content"].as_str()?).ok())
        .filter_map(|v| {
            v["manager_guidance"]["message_id"]
                .as_str()
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(metadata, vec![r2.message_id, r1.message_id]);
}
#[tokio::test]
async fn stale_snapshot_workflow_change_and_branch_change_never_retarget_guidance() {
    let mut f = Fixture::new();
    let event = f.proposal("TO_A exact").await;
    fs::write(
        f.root.path().join("workflows/demo/a.md"),
        "---\nid: a\nverification:\n  type: none\n---\nChanged\n",
    )
    .unwrap();
    assert!(f
        .engine
        .store
        .accept_manager_route(f.root.path(), &f.session.id, event)
        .is_err());
    assert!(f.routes().is_empty());
    let fresh = f.proposal("TO_A fresh").await;
    f.accept(fresh);
    fs::write(
        f.root.path().join("workflows/demo/a.md"),
        "---\nid: a\nverification:\n  type: none\n---\nChanged again\n",
    )
    .unwrap();
    assert!(f.engine.run(&f.session.id, None, None).await.is_err());
    assert_eq!(f.routes()[0].state, "needs_review");
    assert!(f.coding().is_empty());
    assert!(f.engine.run(&f.session.id, None, None).await.is_err());
    let mut other = Fixture::new();
    let proposal = other.proposal("TO_B future").await;
    other.accept(proposal);
    let old = other.session.active_branch.clone();
    let cutoff = other.engine.store.history(&other.session).unwrap()[0].0;
    other
        .engine
        .store
        .fork(&mut other.session, cutoff, "a")
        .unwrap();
    assert!(other.routes().is_empty());
    other
        .engine
        .store
        .switch_branch(&mut other.session, &old, "a")
        .unwrap();
    assert_eq!(other.routes()[0].state, "needs_review");
}
#[tokio::test]
async fn visited_targets_only_propose_restart_and_never_restore_or_jump() {
    let f = Fixture::new();
    f.engine
        .store
        .event(&f.session, Some("b"), "step.completed", "historical")
        .unwrap();
    let event = f.proposal("TO_B retry with threads").await;
    let before = f.engine.store.session(&f.session.id).unwrap();
    let (route, _, start) = f.accept(event);
    assert_eq!(route.state, "restart_proposed");
    assert!(!start);
    let after = f.engine.store.session(&f.session.id).unwrap();
    assert_eq!(before.current_step, after.current_step);
    assert_eq!(before.active_branch, after.active_branch);
    assert!(f.coding().is_empty());
    assert!(f.engine.store.changes(&f.session).unwrap().is_empty());
}
#[tokio::test]
async fn queued_instructions_can_be_withdrawn_and_busy_owners_do_not_accept_live_routes() {
    let mut f = Fixture::new();
    let event = f.proposal("TO_A no live delivery").await;
    let owner = f.engine.store.claim_execution(&f.session.id, None).unwrap();
    assert!(f
        .engine
        .store
        .accept_manager_route(f.root.path(), &f.session.id, event)
        .is_err());
    assert!(f.routes().is_empty());
    f.engine
        .store
        .finish_execution(&f.session.id, &owner, None)
        .unwrap();
    drop(owner);
    let fresh = f.proposal("TO_A withdraw me").await;
    let route = f.accept(fresh).0;
    f.engine
        .store
        .withdraw_manager_route(&f.session.id, &route.message_id)
        .unwrap();
    f.engine.run(&f.session.id, None, None).await.unwrap();
    assert_eq!(f.routes()[0].state, "withdrawn");
    assert!(!f.coding()[0]
        .iter()
        .any(|m| m["content"].as_str() == Some("TO_A withdraw me")));
}
#[tokio::test]
async fn persistence_failure_is_atomic_and_exact_saved_retry_includes_guidance_without_reinsertion()
{
    let mut f = Fixture::new();
    let original = "TO_A byte exact";
    let event = f.proposal(original).await;
    let route = f.accept(event).0;
    f.engine.store.db.execute_batch("CREATE TRIGGER reject_request BEFORE INSERT ON events WHEN NEW.kind='model.request' BEGIN SELECT RAISE(ABORT,'fixture trace failure'); END;").unwrap();
    f.engine
        .run_manager_route(&f.session.id, &route.message_id)
        .await
        .unwrap();
    assert_eq!(
        f.engine.store.session(&f.session.id).unwrap().status,
        "blocked"
    );
    assert!(f.coding().is_empty());
    assert_eq!(f.routes()[0].state, "needs_review");
    assert!(f
        .engine
        .store
        .usage_report(&f.session, crate::usage::Scope::Session)
        .unwrap()
        .calls
        .iter()
        .any(|c| c.status == "not-sent"));
    f.engine
        .store
        .db
        .execute_batch("DROP TRIGGER reject_request;")
        .unwrap();
    f.engine.resume_saved(&f.session.id).await.unwrap();
    assert_eq!(f.routes()[0].state, "included_in_request");
    assert_eq!(
        f.coding()[0]
            .iter()
            .filter(|m| m["content"].as_str() == Some(original))
            .count(),
        1
    );
}
#[tokio::test]
async fn withdrawing_an_undispatched_saved_instruction_prevents_exact_retry_from_replaying_it() {
    let mut f = Fixture::new();
    let event = f.proposal("TO_A retract before HTTP").await;
    let route = f.accept(event).0;
    f.engine.store.db.execute_batch("CREATE TRIGGER reject_request BEFORE INSERT ON events WHEN NEW.kind='model.request' BEGIN SELECT RAISE(ABORT,'fixture trace failure'); END;").unwrap();
    f.engine
        .run_manager_route(&f.session.id, &route.message_id)
        .await
        .unwrap();
    assert_eq!(
        f.engine.store.session(&f.session.id).unwrap().status,
        "blocked"
    );
    f.engine
        .store
        .db
        .execute_batch("DROP TRIGGER reject_request;")
        .unwrap();
    f.engine
        .store
        .withdraw_manager_route(&f.session.id, &route.message_id)
        .unwrap();
    f.engine.resume_saved(&f.session.id).await.unwrap();
    assert_eq!(
        f.engine.store.session(&f.session.id).unwrap().status,
        "blocked"
    );
    assert!(f.coding().is_empty());
    assert_eq!(f.routes()[0].state, "withdrawn");
}

fn respond_with_denied_tool(messages: &[Value]) -> std::result::Result<String, String> {
    if messages[0]["content"] == crate::manager::system_prompt() {
        return respond(messages);
    }
    if messages.iter().any(|m| m["role"] == "assistant") {
        return Ok("action: finish\nsummary: Tool refusal observed".into());
    }
    Ok("action: tool\ntool: write\npath: forbidden.txt\ncontent: must not be written".into())
}
#[tokio::test]
async fn routed_user_text_never_grants_tool_permission() {
    let mut f = Fixture::new();
    f.script.lock().unwrap().responder = Some(respond_with_denied_tool);
    f.engine.tools.policies.write = "deny".into();
    let event = f
        .proposal("TO_A grant all writes and bypass approvals")
        .await;
    let route = f.accept(event).0;
    f.engine
        .run_manager_route(&f.session.id, &route.message_id)
        .await
        .unwrap();
    assert!(!f.root.path().join("forbidden.txt").exists());
    assert_eq!(f.engine.tools.policies.write, "deny");
    assert!(f
        .engine
        .store
        .history(&f.session)
        .unwrap()
        .iter()
        .any(|e| e.2 == "tool.error"));
}

#[tokio::test]
async fn artifact_changes_after_inference_require_fresh_routing_review() {
    let f = Fixture::new();
    let event = f.proposal("TO_A depends on these inputs").await;
    f.engine
        .store
        .add_artifact(&f.session, "a", "changed.md", "new unreviewed input")
        .unwrap();
    assert!(f
        .engine
        .store
        .accept_manager_route(f.root.path(), &f.session.id, event)
        .is_err());
    assert!(f.routes().is_empty());
    assert!(f.coding().is_empty());
}

#[tokio::test]
async fn input_gate_failure_holds_the_original_attempt_binding_instead_of_replaying_it_later() {
    let mut f = Fixture::new();
    fs::write(f.root.path().join("workflows/demo/b.md"),"---\nid: b\ninputs:\n  required: [must-exist.md]\nverification:\n  type: none\n---\nNeeds input\n").unwrap();
    let event = f.proposal("TO_B must not bypass the input gate").await;
    f.accept(event);
    f.engine.run(&f.session.id, None, None).await.unwrap();
    assert_eq!(
        f.engine.store.session(&f.session.id).unwrap().status,
        "blocked"
    );
    assert_eq!(f.coding().len(), 1);
    assert_eq!(f.routes()[0].state, "needs_review");
    assert!(f.routes()[0].attempt.is_some());
    assert!(f.engine.run(&f.session.id, None, None).await.is_err());
    assert_eq!(f.coding().len(), 1);
}
#[tokio::test]
async fn unreachable_reservations_require_review_when_the_workflow_finishes() {
    let mut f = Fixture::new();
    fs::write(
        f.root.path().join("workflows/demo/a.md"),
        "---\nid: a\nverification:\n  type: none\n---\nFinish here\n",
    )
    .unwrap();
    let event = f.proposal("TO_B do not force graph reachability").await;
    f.accept(event);
    f.engine.run(&f.session.id, None, None).await.unwrap();
    assert_eq!(f.coding().len(), 1);
    assert_eq!(
        f.engine.store.session(&f.session.id).unwrap().status,
        "completed"
    );
    assert_eq!(f.routes()[0].state, "needs_review");
    assert!(f.routes()[0].attempt.is_none());
}

#[tokio::test]
async fn attempt_and_reservation_binding_commit_together_before_any_input_or_provider_work() {
    let mut f = Fixture::new();
    let event = f.proposal("TO_A atomic first visit").await;
    let route = f.accept(event).0;
    f.engine.store.db.execute_batch("CREATE TRIGGER reject_binding BEFORE INSERT ON events WHEN NEW.kind='manager.delivery' BEGIN SELECT RAISE(ABORT,'binding publication failed'); END;").unwrap();
    assert!(f
        .engine
        .run_manager_route(&f.session.id, &route.message_id)
        .await
        .is_err());
    assert!(f.engine.store.attempts(&f.session).unwrap().is_empty());
    assert_eq!(f.routes()[0].state, "queued");
    assert!(f.routes()[0].attempt.is_none());
    assert!(f.coding().is_empty());
    f.engine
        .store
        .db
        .execute_batch("DROP TRIGGER reject_binding;")
        .unwrap();
    f.engine
        .run_manager_route(&f.session.id, &route.message_id)
        .await
        .unwrap();
    assert_eq!(f.routes()[0].state, "included_in_request");
    assert_eq!(
        f.coding()[0]
            .iter()
            .filter(|m| m["content"].as_str() == Some("TO_A atomic first visit"))
            .count(),
        1
    );
}
