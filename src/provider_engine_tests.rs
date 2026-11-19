//! Engine retries use mock completions; no paid API calls or credentials.
use super::*;
use crate::{
    provider::{Completion, Script},
    provider_failure::Kind,
    usage::Scope,
};
use std::{
    fs,
    sync::{atomic::Ordering, Arc, Mutex},
};
fn fixture(responses: Vec<Completion>) -> (tempfile::TempDir, Engine, Session) {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("greenlight.yaml"),
        "provider:\n  model: fixture\n",
    )
    .unwrap();
    let mut engine = Engine::open(root.path()).unwrap();
    engine.provider.script = Some(Arc::new(Mutex::new(Script {
        completions: responses.into(),
        ..Default::default()
    })));
    let s = engine.store.create("demo", "goal", "work").unwrap();
    engine
        .store
        .begin_attempt(&s, "work", "id: work", "fixture")
        .unwrap();
    (root, engine, s)
}
fn rate(wait: u64) -> Completion {
    let mut c = Completion::parse(
        429,
        r#"{"error":{"code":"rate_limit_exceeded","message":"fixture-secret"}}"#,
    )
    .unwrap();
    let f = c.failure.as_mut().unwrap();
    f.retry_after_seconds = Some(wait);
    c.content = Err(f.to_string());
    c
}
fn success() -> Completion {
    Completion::parse(200,r#"{"choices":[{"message":{"content":"done"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}"#).unwrap()
}
#[tokio::test]
async fn rate_limit_retries_have_distinct_durable_usage_records() {
    let (_root, engine, s) = fixture(vec![rate(0), success()]);
    assert_eq!(
        engine
            .model_call(&s, "work", "coding", "fixture", &[])
            .await
            .unwrap(),
        "done"
    );
    let report = engine.store.usage_report(&s, Scope::Session).unwrap();
    assert_eq!(report.totals.requests, 2);
    assert_ne!(report.calls[0].id, report.calls[1].id);
    assert!(report.calls.iter().all(|c| c.attempt.is_some()));
    assert_eq!(report.totals.output.reported, 2);
    assert_eq!(report.totals.output.unknown_requests, 1);
    assert!(engine
        .store
        .history(&s)
        .unwrap()
        .iter()
        .any(|(_, _, kind, _)| kind == "provider.retry"));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("fixture-secret"));
}
#[tokio::test]
async fn auth_billing_unknown_rate_and_service_failures_never_retry() {
    for (status, body) in [
        (
            401,
            r#"{"error":{"code":"invalid_api_key","message":"fixture-secret"}}"#,
        ),
        (429, r#"{"error":{"code":"insufficient_quota"}}"#),
        (429, r#"{"error":{"message":"fixture-secret"}}"#),
        (503, "fixture-secret"),
    ] {
        let (_root, engine, s) = fixture(vec![Completion::parse(status, body).unwrap(), success()]);
        let error = engine
            .model_call(&s, "work", "coding", "fixture", &[])
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("fixture-secret"));
        assert_eq!(
            engine
                .store
                .usage_report(&s, Scope::Session)
                .unwrap()
                .totals
                .requests,
            1
        );
        assert_eq!(
            engine
                .provider
                .script
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .requests
                .len(),
            1
        );
        assert!(!serde_json::to_string(&engine.store.history(&s).unwrap())
            .unwrap()
            .contains("fixture-secret"));
    }
}
#[tokio::test]
async fn retries_stop_at_three_requests_or_excessive_server_wait() {
    let (_root, engine, s) = fixture(vec![rate(0), rate(0), rate(0), success()]);
    assert!(engine
        .model_call(&s, "work", "coding", "fixture", &[])
        .await
        .unwrap_err()
        .chain()
        .any(|cause| cause.to_string().contains("budget")));
    assert_eq!(
        engine
            .store
            .usage_report(&s, Scope::Session)
            .unwrap()
            .totals
            .requests,
        3
    );
    let (_root, engine, s) = fixture(vec![rate(60), success()]);
    assert!(format!(
        "{:#}",
        engine
            .model_call(&s, "work", "coding", "fixture", &[])
            .await
            .unwrap_err()
    )
    .contains("60s"));
    assert_eq!(
        engine
            .store
            .usage_report(&s, Scope::Session)
            .unwrap()
            .totals
            .requests,
        1
    );
}
#[tokio::test]
async fn cancel_during_wait_records_no_second_request() {
    let (_root, engine, s) = fixture(vec![rate(15), success()]);
    let cancel = engine.provider.retry_cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cancel.store(true, Ordering::SeqCst);
    });
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        engine.model_call(&s, "work", "coding", "fixture", &[]),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<crate::provider_failure::Failure>()
            .unwrap()
            .kind,
        Kind::Cancelled
    );
    assert_eq!(
        engine
            .store
            .usage_report(&s, Scope::Session)
            .unwrap()
            .totals
            .requests,
        1
    );
    assert!(engine
        .store
        .history(&s)
        .unwrap()
        .iter()
        .any(|(_, _, kind, _)| kind == "provider.retry-cancelled"));
}

#[tokio::test]
async fn provider_failure_during_verification_blocks_without_workflow_retry_or_detector() {
    let coding = Completion::parse(
        200,
        r#"{"choices":[{"message":{"content":"action: finish"}}]}"#,
    )
    .unwrap();
    let rejected = Completion::parse(
        401,
        r#"{"error":{"code":"invalid_api_key","message":"fixture-secret"}}"#,
    )
    .unwrap();
    let (root, mut engine, s) = fixture(vec![coding, rejected, success()]);
    let folder = root.path().join("workflows/demo");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
    )
    .unwrap();
    fs::write(folder.join("work.md"),"---\nid: work\nverification:\n  type: agent\n  prompt: review.md\ndeviations:\n  - id: verification_failed\n    action: retry\n    max_retries: 3\n    prompt: review.md\n---\nWork\n").unwrap();
    fs::write(folder.join("review.md"), "Review").unwrap();
    engine.run(&s.id, None, None).await.unwrap();
    assert_eq!(engine.store.session(&s.id).unwrap().status, "blocked");
    assert_eq!(
        engine
            .provider
            .script
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .requests
            .len(),
        2
    );
    let history = engine.store.history(&s).unwrap();
    assert!(history
        .iter()
        .any(|(_, _, kind, body)| kind == "step.blocked" && body.contains("credentials")));
    assert!(!history
        .iter()
        .any(|(_, _, kind, _)| kind == "step.deviation"));
    assert!(!serde_json::to_string(&history)
        .unwrap()
        .contains("fixture-secret"));
}

#[tokio::test]
async fn local_auth_failure_is_not_sent_and_pre_cancel_creates_no_request() {
    let (_root, mut engine, s) = fixture(vec![]);
    engine.provider.script = None;
    engine.provider.cfg.api_key_env =
        format!("GREENLIGHT_MISSING_{}", uuid::Uuid::new_v4().simple());
    let error = engine
        .model_call(&s, "work", "coding", "fixture", &[])
        .await
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<crate::provider_failure::Failure>()
            .unwrap()
            .kind,
        Kind::Credentials
    );
    let report = engine.store.usage_report(&s, Scope::Session).unwrap();
    assert_eq!(report.calls[0].status, "not-sent");
    assert!(report.calls[0]
        .counts
        .notes
        .iter()
        .any(|note| note.contains("No HTTP request was sent")));
    let (_root, engine, s) = fixture(vec![success()]);
    engine.provider.retry_cancel.store(true, Ordering::SeqCst);
    assert!(engine
        .model_call(&s, "work", "coding", "fixture", &[])
        .await
        .is_err());
    assert_eq!(
        engine
            .store
            .usage_report(&s, Scope::Session)
            .unwrap()
            .totals
            .requests,
        0
    );
}

#[tokio::test]
async fn usage_persistence_failure_never_schedules_a_second_request() {
    let (_root, engine, s) = fixture(vec![rate(0), success()]);
    engine.store.db.execute_batch("CREATE TRIGGER reject_usage_update BEFORE UPDATE ON model_calls BEGIN SELECT RAISE(ABORT,'injected ledger failure'); END;").unwrap();
    let error = engine
        .model_call(&s, "work", "coding", "fixture", &[])
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("could not persist model usage"));
    assert_eq!(
        engine
            .provider
            .script
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .requests
            .len(),
        1
    );
    let report = engine.store.usage_report(&s, Scope::Session).unwrap();
    assert_eq!(report.totals.requests, 1);
    assert_eq!(report.calls[0].status, "started-or-interrupted");
    assert!(!engine
        .store
        .history(&s)
        .unwrap()
        .iter()
        .any(|(_, _, kind, _)| kind == "provider.retry"));
}

fn verification_workflow(root: &Path, kind: &str) {
    let folder = root.join("workflows/demo");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
    )
    .unwrap();
    fs::write(folder.join("work.md"),format!("---\nid: work\nverification:\n  type: {kind}\n  checks:\n    - type: agent\n      prompt: review.md\n    - type: agent\n      prompt: review.md\ndeviations:\n  - id: verification_failed\n    action: retry\n    max_retries: 3\n    prompt: review.md\n---\nWork\n")).unwrap();
    fs::write(folder.join("review.md"), "Review fixture").unwrap();
}
fn text_completion(content: &str) -> Completion {
    Completion::parse(
        200,
        &serde_json::json!({"choices":[{"message":{"content":content}}]}).to_string(),
    )
    .unwrap()
}

#[tokio::test]
async fn any_verification_provider_failure_stops_before_fallback_or_detector() {
    let rejected = Completion::parse(401, r#"{"error":{"code":"invalid_api_key"}}"#).unwrap();
    let (root, mut engine, s) = fixture(vec![
        text_completion("action: finish"),
        rejected,
        text_completion("passed: true\nreason: should not be called"),
    ]);
    verification_workflow(root.path(), "any");
    engine.run(&s.id, None, None).await.unwrap();
    assert_eq!(engine.store.session(&s.id).unwrap().status, "blocked");
    assert_eq!(
        engine
            .provider
            .script
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .requests
            .len(),
        2
    );
    let history = engine.store.history(&s).unwrap();
    assert!(!history.iter().any(|e| matches!(
        e.2.as_str(),
        "step.deviation" | "verification.passed" | "deviation.detector"
    )));
}

#[tokio::test]
async fn verification_usage_failure_is_fatal_for_any_and_all_policies() {
    for kind in ["any", "all"] {
        let (root, mut engine, s) = fixture(vec![
            text_completion("action: finish"),
            text_completion("passed: true\nreason: accepted"),
            text_completion("passed: true\nreason: must not repeat"),
        ]);
        verification_workflow(root.path(), kind);
        engine.store.db.execute_batch("CREATE TRIGGER reject_verification_usage BEFORE UPDATE ON model_calls WHEN json_extract(OLD.payload,'$.purpose')='verification' BEGIN SELECT RAISE(ABORT,'fixture accounting failure'); END;").unwrap();
        engine.run(&s.id, None, None).await.unwrap();
        assert_eq!(engine.store.session(&s.id).unwrap().status, "blocked");
        assert_eq!(
            engine
                .provider
                .script
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .requests
                .len(),
            2,
            "{kind}"
        );
        let history = engine.store.history(&s).unwrap();
        assert!(!history.iter().any(|e| matches!(
            e.2.as_str(),
            "step.deviation" | "verification.passed" | "deviation.detector"
        )));
        let usage = engine.store.usage_report(&s, Scope::Session).unwrap();
        assert_eq!(usage.totals.requests, 2);
        assert_eq!(usage.calls[1].status, "started-or-interrupted");
    }
}

#[tokio::test]
async fn any_verification_still_accepts_later_check_after_a_genuine_rejection() {
    let (root, mut engine, s) = fixture(vec![
        text_completion("action: finish"),
        text_completion("passed: false\nreason: rejected"),
        text_completion("passed: true\nreason: alternate check passed"),
    ]);
    verification_workflow(root.path(), "any");
    engine.run(&s.id, None, None).await.unwrap();
    assert_eq!(engine.store.session(&s.id).unwrap().status, "completed");
    assert_eq!(
        engine
            .provider
            .script
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .requests
            .len(),
        3
    );
    assert!(engine
        .store
        .history(&s)
        .unwrap()
        .iter()
        .any(|e| e.2 == "verification.passed"));
}

#[tokio::test]
async fn conversation_dump_matches_exact_dispatched_messages_for_every_model_purpose() {
    let (_root, engine, s) = fixture(vec![success(), success(), success(), success()]);
    let messages = vec![
        serde_json::json!({"role":"system","content":"protocol"}),
        serde_json::json!({"role":"user","content":"Tool error: shell is disabled\nRetry with read"}),
    ];
    for purpose in ["coding", "verification", "deviation-detector", "manager"] {
        engine
            .model_call(&s, "work", purpose, "fixture", &messages)
            .await
            .unwrap();
    }
    let dump = crate::conversation_dump::Dump::load(&engine.store, &s.id, None, false).unwrap();
    let recorded = dump
        .events
        .iter()
        .filter(|e| e.kind == "model.request")
        .map(|e| {
            let event: serde_json::Value = serde_json::from_str(&e.data).unwrap();
            event["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| dump.messages[id.as_str().unwrap()].clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        recorded,
        engine
            .provider
            .script
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .requests
    );
    assert_eq!(
        dump.events
            .iter()
            .filter(|e| e.kind == "model.response")
            .count(),
        4
    );
    assert_eq!(dump.messages.len(), 3);
    assert!(!dump
        .diagnostics
        .iter()
        .any(|d| d.contains("Legacy run") || d.contains("no response trace")));
}

#[tokio::test]
async fn failed_trace_persistence_prevents_http_dispatch() {
    let (root, engine, s) = fixture(vec![success()]);
    let blobs = engine.store.directory.join("blobs");
    if blobs.exists() {
        std::fs::remove_dir(&blobs).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path(), &blobs).unwrap();
    #[cfg(not(unix))]
    std::fs::write(&blobs, "not a directory").unwrap();
    let error = engine
        .model_call(
            &s,
            "work",
            "coding",
            "fixture",
            &[serde_json::json!({"role":"user","content":"test"})],
        )
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("request not sent"));
    assert!(engine
        .provider
        .script
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .requests
        .is_empty());
    let report = engine.store.usage_report(&s, Scope::Session).unwrap();
    assert_eq!(report.calls[0].status, "not-sent");
}

#[test]
fn manual_provider_retries_preserve_all_call_purposes_and_stop_does_not_dispatch() {
    for (purpose, status, body, retry) in [
        ("verification", 503, "{}", true),
        ("deviation-detector", 401, "{}", true),
        ("coding", 402, "{}", true),
        ("coding", 429, "{}", false),
        ("coding", 429, r#"{"usage":{"prompt_tokens":12}}"#, false),
    ] {
        let failure = Completion::parse(status, body).unwrap();
        let (_root, mut engine, s) = fixture(vec![failure, success()]);
        engine.interactive_provider_retry = true;
        let (pt, requests) = std::sync::mpsc::channel();
        let (answers, rx) = std::sync::mpsc::channel();
        engine.tools.use_tui(pt, rx);
        let script = engine.provider.script.clone().unwrap();
        let session = s.clone();
        let messages = vec![serde_json::json!({"role":"user","content":"exact original messages"})];
        let worker = std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(engine.model_call(&s, "work", purpose, "fixture", &messages));
            (result, engine)
        });
        let prompt = requests
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert_eq!(prompt.mode, crate::tools::ApprovalMode::ProviderRetry);
        assert!(prompt.content.contains("same request"));
        if status == 503 || body.contains("usage") {
            assert!(prompt.content.contains("billing remains uncertain"));
        }
        answers.send(if retry { "y" } else { "n" }.into()).unwrap();
        let (result, engine) = worker.join().unwrap();
        assert_eq!(result.is_ok(), retry);
        let script = script.lock().unwrap();
        assert_eq!(script.requests.len(), if retry { 2 } else { 1 });
        if retry {
            assert_eq!(script.requests[0], script.requests[1]);
        }
        assert_eq!(engine.store.attempts(&session).unwrap().len(), 1);
    }
}
