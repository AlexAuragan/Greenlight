//! Deterministic engine regression tests; no credentials or network calls.
use crate::{engine::Engine, provider::Script};
use std::{
    fs,
    sync::{Arc, Mutex},
};

const REPORT: &str = "action: deviation\ndeviation_id: blocker\nsummary: fixture evidence";
const ACCEPT: &str = "detected: true\nreason: supported";
const REJECT: &str = "detected: false\nreason: unsupported";
const FINISH: &str = "action: finish\nsummary: complete";

struct Harness {
    root: tempfile::TempDir,
    engine: Engine,
    session: String,
    script: Arc<Mutex<Script>>,
}

impl Harness {
    fn new(rule: &str, responses: Vec<Result<&str, &str>>) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("greenlight.yaml"), "provider:\n  model: fixture\n  api_key_env: GREENLIGHT_TEST_UNUSED_KEY\n  base_url: http://127.0.0.1:9\n").unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: step\nsteps: [step]\n",
        )
        .unwrap();
        fs::write(folder.join("step.md"), format!("---\nid: step\ntools: []\nverification:\n  type: none\ndeviations:\n{rule}\n---\nFixture\n")).unwrap();
        fs::write(
            folder.join("detector.md"),
            "DETECTOR_POLICY_MARKER: evaluate the evidence",
        )
        .unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        let session = engine
            .store
            .create("demo", "fixture goal", "step")
            .unwrap()
            .id;
        let script = Arc::new(Mutex::new(Script {
            responses: responses
                .into_iter()
                .map(|r| r.map(str::to_owned).map_err(str::to_owned))
                .collect(),
            requests: Vec::new(),
        }));
        engine.provider.script = Some(script.clone());
        Self {
            root,
            engine,
            session,
            script,
        }
    }
    async fn run(&mut self) {
        self.engine.run(&self.session, None, None).await.unwrap();
    }
    fn count(&self, kind: &str) -> usize {
        let session = self.engine.store.session(&self.session).unwrap();
        self.engine
            .store
            .history(&session)
            .unwrap()
            .iter()
            .filter(|(_, _, k, _)| k == kind)
            .count()
    }
    fn status(&self) -> String {
        self.engine.store.session(&self.session).unwrap().status
    }
    fn assert_calls(&self, count: usize) {
        let script = self.script.lock().unwrap();
        assert_eq!(script.requests.len(), count);
        assert!(script.responses.is_empty());
    }
}

const PAUSE: &str = "  - id: blocker\n    action: pause\n    prompt: detector.md";

#[tokio::test]
async fn accepted_report_is_detected_once_before_pause() {
    let mut h = Harness::new(PAUSE, vec![Ok(REPORT), Ok(ACCEPT)]);
    h.run().await;
    h.assert_calls(2);
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("deviation.detector"), 1);
    assert_eq!(h.count("deviation.accepted"), 1);
    assert_eq!(h.count("step.deviation"), 1);
    assert!(h.script.lock().unwrap().requests[1][0]["content"]
        .as_str()
        .unwrap()
        .contains("DETECTOR_POLICY_MARKER"));
}

#[tokio::test]
async fn rejected_report_returns_to_same_conversation_without_routing() {
    let mut h = Harness::new(PAUSE, vec![Ok(REPORT), Ok(REJECT), Ok(FINISH)]);
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.status(), "completed");
    assert_eq!(h.count("deviation.rejected"), 1);
    assert_eq!(h.count("step.deviation"), 0);
    let script = h.script.lock().unwrap();
    assert!(script.requests[2].iter().any(|m| m["content"]
        .as_str()
        .is_some_and(|c| c.contains("was rejected by the independent detector"))));
}

#[tokio::test]
async fn malformed_or_failed_detector_blocks_without_routing_or_retry() {
    for response in [Ok("not a verdict"), Err("injected transport failure")] {
        let mut h = Harness::new(PAUSE, vec![Ok(REPORT), response]);
        h.run().await;
        h.assert_calls(2);
        assert_eq!(h.status(), "blocked");
        assert_eq!(h.count("step.error"), 1);
        assert_eq!(h.count("step.blocked"), 1);
        assert_eq!(h.count("step.deviation"), 0);
        assert_eq!(h.count("deviation.accepted"), 0);
    }
}

#[tokio::test]
async fn retries_use_one_verdict_per_report_and_keep_the_limit() {
    let rule = "  - id: blocker\n    action: retry\n    max_retries: 1\n    prompt: detector.md";
    let mut h = Harness::new(rule, vec![Ok(REPORT), Ok(ACCEPT), Ok(REPORT), Ok(ACCEPT)]);
    h.run().await;
    h.assert_calls(4);
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("step.started"), 2);
    assert_eq!(h.count("deviation.detector"), 2);
    assert_eq!(h.count("step.deviation"), 2);
}

#[tokio::test]
async fn accepted_report_routes_to_the_declared_target_once() {
    let rule = "  - id: blocker\n    action: goto\n    target: next\n    prompt: detector.md";
    let mut h = Harness::new(rule, vec![Ok(REPORT), Ok(ACCEPT), Ok(FINISH)]);
    let folder = h.root.path().join("workflows/demo");
    fs::write(
        folder.join("workflow.yaml"),
        "id: demo\ntitle: Demo\nstart: step\nsteps: [step, next]\n",
    )
    .unwrap();
    fs::write(
        folder.join("next.md"),
        "---\nid: next\nverification:\n  type: none\n---\nNext step\n",
    )
    .unwrap();
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.status(), "completed");
    assert_eq!(h.count("step.deviation"), 1);
    assert_eq!(h.count("deviation.detector"), 1);
    assert_eq!(
        h.engine.store.session(&h.session).unwrap().current_step,
        "next"
    );
}

#[tokio::test]
async fn unknown_reports_and_rules_without_prompts_do_not_invoke_detectors() {
    for rule in [
        "  - id: blocker\n    action: pause",
        "  - id: different\n    action: pause\n    prompt: detector.md",
    ] {
        let mut h = Harness::new(rule, vec![Ok(REPORT)]);
        h.run().await;
        h.assert_calls(1);
        assert_eq!(h.status(), "blocked");
        assert_eq!(h.count("deviation.detector"), 0);
    }
}

#[tokio::test]
async fn engine_generated_failures_still_evaluate_their_detector_once() {
    for response in [
        Ok(ACCEPT),
        Ok(REJECT),
        Ok("malformed"),
        Err("injected failure"),
    ] {
        let rule = "  - id: missing_artifact\n    action: pause\n    prompt: detector.md";
        let mut h = Harness::new(rule, vec![Ok(FINISH), response]);
        let file = h.root.path().join("workflows/demo/step.md");
        let text = fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: []\noutputs:\n  required: [report.md]");
        fs::write(file, text).unwrap();
        h.run().await;
        h.assert_calls(2);
        assert_eq!(h.status(), "blocked");
        assert_eq!(h.count("step.blocked"), 1);
        assert_eq!(h.count("step.deviation"), 1);
    }
}
