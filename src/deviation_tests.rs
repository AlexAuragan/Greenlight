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
            ..Default::default()
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
async fn activity_titles_link_each_response_without_authorizing_tools_or_extra_calls() {
    let mut h = Harness::new(PAUSE, vec![
        Ok("action: tool\ntool: write\npath: forbidden.txt\ncontent: forbidden\nactivity_title: Testing first fix"),
        Ok("action: artifact\nname: notes.md\ncontent: evidence\nactivity_title: Recording findings"),
        Ok("action: finish\nsummary: done\nactivity_title: Wrapping up"),
    ]);
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: [write]"),
    )
    .unwrap();
    h.engine.tools.policies.write = "deny".into();
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.status(), "completed");
    assert_eq!(h.count("activity.title"), 3);
    assert_eq!(h.count("tool.error"), 1);
    assert_eq!(h.count("file.changed"), 0);
    assert!(!h.root.path().join("forbidden.txt").exists());
    let s = h.engine.store.session(&h.session).unwrap();
    let history = h.engine.store.history(&s).unwrap();
    let projection = crate::activity_cards::Projection::build(&history);
    assert!(projection.malformed.is_empty());
    assert_eq!(projection.titles.len(), 3);
    for (id, evidence) in &projection.titles {
        assert!(evidence.annotation.model_event < *id);
        assert_eq!(projection.cards[id].tone, crate::activity_cards::Tone::Info);
        assert!(projection.cards[id]
            .summary
            .contains("not execution evidence"));
    }
    let title = history.iter().find(|e| e.2 == "activity.title").unwrap();
    let request = history.iter().find(|e| e.2 == "tool.request").unwrap();
    assert!(title.0 < request.0);
    assert!(history
        .iter()
        .any(|e| e.2 == "tool.error" && e.3.contains("forbidden by policy")));
    assert!(request.3.contains("activity_title: Testing first fix"));
    let script = h.script.lock().unwrap();
    assert!(script.requests[0][0]["content"]
        .as_str()
        .unwrap()
        .contains("optional activity_title"));
}

#[tokio::test]
async fn invalid_title_retries_protocol_and_cannot_hide_failed_verification() {
    let mut h = Harness::new(PAUSE, vec![
        Ok("action: tool\ntool: write\npath: forbidden.txt\ncontent: forbidden\nactivity_title: \"bad\\u001b[2J\""),
        Ok("action: finish\nactivity_title: All checks passed"),
        Ok("passed: false\nreason: missing evidence"),
    ]);
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("type: none", "type: agent\n  prompt: detector.md"),
    )
    .unwrap();
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.count("protocol.error"), 1);
    assert_eq!(h.count("activity.title"), 1);
    assert_eq!(h.count("tool.request"), 0);
    assert!(!h.root.path().join("forbidden.txt").exists());
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("verification.passed"), 0);
    assert_eq!(h.count("verification.agent"), 1);
    let s = h.engine.store.session(&h.session).unwrap();
    let projection = crate::activity_cards::Projection::build(&h.engine.store.history(&s).unwrap());
    assert!(projection.malformed.is_empty());
    let title = *projection.titles.keys().next().unwrap();
    assert_eq!(
        projection.cards[&title].tone,
        crate::activity_cards::Tone::Info
    );
    let script = h.script.lock().unwrap();
    assert!(!script.requests[2]
        .iter()
        .any(|m| m["content"].as_str().unwrap().contains("All checks passed")));
}

#[tokio::test]
async fn deviation_title_remains_annotation_and_is_not_detector_authority() {
    let mut h = Harness::new(PAUSE, vec![
        Ok("action: deviation\ndeviation_id: blocker\nsummary: evidence\nactivity_title: Examining blocker"),
        Ok(REJECT), Ok(FINISH),
    ]);
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.count("activity.title"), 1);
    assert_eq!(h.count("deviation.rejected"), 1);
    assert_eq!(h.status(), "completed");
    let script = h.script.lock().unwrap();
    assert!(!script.requests[1]
        .iter()
        .any(|m| m["content"].as_str().unwrap().contains("Examining blocker")));
}

#[tokio::test]
async fn reviewer_uses_only_current_attempt_evidence_and_records_versions() {
    let artifact = "action: artifact\nname: report.md\ncontent: CURRENT_REPORT";
    let mut h = Harness::new(
        PAUSE,
        vec![
            Ok("action: tool\ntool: read\npath: observation.txt"),
            Ok(artifact),
            Ok(FINISH),
            Ok("passed: true\nreason: fixture"),
        ],
    );
    let session = h.engine.store.session(&h.session).unwrap();
    h.engine
        .store
        .begin_attempt(&session, "step", "old definition", "fixture")
        .unwrap();
    h.engine
        .store
        .add_artifact(&session, "step", "obsolete.md", "OLD_REPORT")
        .unwrap();
    h.engine
        .store
        .event(&session, Some("step"), "tool.result", "OLD_TOOL_SUCCESS")
        .unwrap();
    h.engine
        .store
        .event(&session, Some("step"), "step.blocked", "retry")
        .unwrap();
    let file = h.root.path().join("workflows/demo/step.md");
    let text = fs::read_to_string(&file)
        .unwrap()
        .replace("type: none", "type: agent\n  prompt: detector.md")
        .replace("tools: []", "tools: [read]");
    fs::write(file, text).unwrap();
    fs::write(
        h.root.path().join("observation.txt"),
        format!("{}CURRENT_TOOL_FAILURE", "line\n".repeat(12)),
    )
    .unwrap();
    h.run().await;
    h.assert_calls(4);
    assert_eq!(h.status(), "completed");
    let script = h.script.lock().unwrap();
    let query = script.requests[3][0]["content"].as_str().unwrap();
    assert!(query.contains("CURRENT_REPORT"));
    assert!(query.contains("CURRENT_TOOL_FAILURE"));
    assert!(!query.contains("OLD_REPORT"));
    assert!(!query.contains("OLD_TOOL_SUCCESS"));
    let session = h.engine.store.session(&h.session).unwrap();
    let history = h.engine.store.history(&session).unwrap();
    let manifest = history
        .iter()
        .find(|(_, _, kind, _)| kind == "verification.evidence")
        .unwrap();
    let evidence: serde_json::Value = serde_json::from_str(&manifest.3).unwrap();
    assert_eq!(evidence["artifact_events"].as_array().unwrap().len(), 1);
    assert_eq!(evidence["observation_events"].as_array().unwrap().len(), 1);
    assert_eq!(h.engine.store.attempt_history(&session).unwrap().len(), 2);
}

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

#[tokio::test]
async fn agent_continues_past_legacy_step_workflow_and_session_turn_limits() {
    let mut responses = vec![
        Ok("not a YAML action"),
        Ok("action: tool\ntool: read\npath: unavailable.txt"),
    ];
    responses.extend(std::iter::repeat_n(
        Ok("action: artifact\nname: notes.md\ncontent: next observation"),
        20,
    ));
    responses.push(Ok(FINISH));
    let mut h = Harness::new("  - id: blocker\n    action: pause", responses);
    h.engine.cfg.session.max_turns_per_attempt = 1;
    let folder = h.root.path().join("workflows/demo");
    let workflow = folder.join("workflow.yaml");
    fs::write(
        &workflow,
        fs::read_to_string(&workflow).unwrap() + "defaults:\n  max_turns: 2\n",
    )
    .unwrap();
    let step = folder.join("step.md");
    fs::write(
        &step,
        fs::read_to_string(&step)
            .unwrap()
            .replacen("id: step\n", "id: step\nmax_turns: 3\n", 1),
    )
    .unwrap();
    h.run().await;
    assert_eq!(h.status(), "completed");
    h.assert_calls(23);
    assert_eq!(h.count("protocol.error"), 1);
    assert_eq!(h.count("tool.error"), 1);
    assert_eq!(h.count("artifact.created"), 20);
    assert_eq!(h.count("step.deviation"), 0);
}

#[tokio::test]
async fn unlimited_agent_still_stops_before_dispatch_when_cancelled() {
    let mut h = Harness::new("  - id: blocker\n    action: pause", vec![Ok(FINISH)]);
    h.engine
        .provider
        .retry_cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);
    h.run().await;
    assert_eq!(h.status(), "blocked");
    assert!(h.script.lock().unwrap().requests.is_empty());
    assert_eq!(h.script.lock().unwrap().responses.len(), 1);
    assert_eq!(h.count("step.completed"), 0);
    assert_eq!(h.count("step.error"), 1);
}

#[tokio::test]
async fn format_correction_reaches_the_model_and_a_corrected_read_really_executes() {
    fn respond(messages: &[serde_json::Value]) -> Result<String, String> {
        let last = messages.last().unwrap()["content"].as_str().unwrap();
        match messages.len() {
            2 => {
                assert!(messages[0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("read, write, shell and web are TOOL NAMES"));
                assert!(last.contains("Workflow-enabled tool names: read"));
                Ok("action: read\npath: notes.md".into())
            }
            4 => {
                assert!(last.contains("protocol format error"));
                assert!(last.contains("no tool was attempted"));
                assert!(last.contains("action: tool\ntool: read"));
                assert!(last.contains("NOT evidence that tools are unavailable"));
                Ok("action: tool\ntool: read\npath: notes.md".into())
            }
            6 => {
                assert!(last.contains("Tool result:\nACTUAL_FILE_BODY"));
                Ok(FINISH.into())
            }
            _ => Err("unexpected request".into()),
        }
    }
    let mut h = Harness::new("  - id: blocker\n    action: pause", vec![]);
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: [read]"),
    )
    .unwrap();
    fs::write(h.root.path().join("notes.md"), "ACTUAL_FILE_BODY").unwrap();
    h.script.lock().unwrap().responder = Some(respond);
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.status(), "completed");
    assert_eq!(h.count("protocol.error"), 1);
    assert_eq!(h.count("tool.request"), 1);
    assert_eq!(h.count("tool.result"), 1);
    assert_eq!(h.count("tool.error"), 0);
}

#[tokio::test]
async fn correcting_request_syntax_never_bypasses_host_denial_or_workflow_tool_selection() {
    for denied_by_host in [true, false] {
        let mut h = Harness::new(
            "  - id: blocker\n    action: pause",
            vec![
                Ok("action: read\npath: notes.md"),
                Ok("action: tool\ntool: read\npath: notes.md"),
                Ok(FINISH),
            ],
        );
        fs::write(h.root.path().join("notes.md"), "MUST_NOT_REACH_MODEL").unwrap();
        if denied_by_host {
            let file = h.root.path().join("workflows/demo/step.md");
            fs::write(
                &file,
                fs::read_to_string(&file)
                    .unwrap()
                    .replace("tools: []", "tools: [read]"),
            )
            .unwrap();
            h.engine.tools.policies.read = "deny".into();
        }
        h.run().await;
        h.assert_calls(3);
        assert_eq!(h.count("protocol.error"), 1);
        assert_eq!(h.count("tool.request"), 1);
        assert_eq!(h.count("tool.result"), 0);
        assert_eq!(h.count("tool.error"), 1);
        let script = h.script.lock().unwrap();
        let last = script.requests[2].last().unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(!last.contains("MUST_NOT_REACH_MODEL"));
        assert!(last.contains("action format was accepted"));
        assert!(last.contains("Do not bypass a permission denial"));
        assert!(last.contains(if denied_by_host {
            "read forbidden by policy"
        } else {
            "not enabled for this step"
        }));
        let dump =
            crate::conversation_dump::Dump::load(&h.engine.store, &h.session, None, false).unwrap();
        let recorded = dump
            .events
            .iter()
            .filter(|e| e.kind == "model.request")
            .map(|e| {
                let value: serde_json::Value = serde_json::from_str(&e.data).unwrap();
                value["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| dump.messages[id.as_str().unwrap()].clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(recorded, script.requests);
        assert!(dump.markdown().contains(if denied_by_host {
            "read forbidden by policy"
        } else {
            "not enabled for this step"
        }));
        assert!(dump
            .diagnostics
            .iter()
            .any(|d| d.contains("protocol parser")));
        assert!(dump
            .diagnostics
            .iter()
            .any(|d| d.contains("Tool action failed")));
    }
}

#[tokio::test]
async fn directory_discovery_and_wrapped_actions_find_real_source_with_shell_denied() {
    fn respond(messages: &[serde_json::Value]) -> Result<String, String> {
        let last = messages.last().unwrap()["content"].as_str().unwrap();
        match messages.len() {
            2 => {
                assert!(last.contains("Native file discovery"));
                Ok(
                    "I will inspect the project.\n```yaml\naction: tool\ntool: read\npath: .\n```"
                        .into(),
                )
            }
            4 => {
                let listing: serde_json::Value =
                    serde_json::from_str(last.strip_prefix("Tool result:\n").unwrap()).unwrap();
                assert!(listing["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["path"] == "widgets/bar.py"));
                assert!(!last.contains(".env"));
                Ok("Inspecting the source.</think>\n```yaml\naction: tool\ntool: read\npath: widgets/bar.py\n```".into())
            }
            6 => {
                assert!(last.contains("BAR_CLOCK_SOURCE"));
                Ok("Done.\n```yaml\naction: finish\nsummary: source inspected\n```".into())
            }
            _ => Err("unexpected request".into()),
        }
    }
    let mut h = Harness::new("  - id: blocker\n    action: pause", vec![]);
    let step = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &step,
        fs::read_to_string(&step)
            .unwrap()
            .replace("tools: []", "tools: [read]"),
    )
    .unwrap();
    fs::create_dir(h.root.path().join("widgets")).unwrap();
    fs::write(h.root.path().join("widgets/bar.py"), "BAR_CLOCK_SOURCE").unwrap();
    fs::write(h.root.path().join(".env"), "PRIVATE").unwrap();
    h.engine.tools.policies.shell = "deny".into();
    h.script.lock().unwrap().responder = Some(respond);
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.status(), "completed");
    assert_eq!(h.count("protocol.error"), 0);
    assert_eq!(h.count("tool.result"), 2);
    assert_eq!(h.count("tool.error"), 0);
    let s = h.engine.store.session(&h.session).unwrap();
    let history = h.engine.store.history(&s).unwrap();
    assert!(!history
        .iter()
        .any(|e| e.2 == "permission.decision" && e.3.starts_with("shell:")));
    assert!(
        history
            .iter()
            .any(|e| e.2 == "model.output" && e.3.starts_with("I will inspect")),
        "original response must remain recorded"
    );
}

#[tokio::test]
async fn missing_action_fields_are_repaired_and_ambiguous_fences_never_execute() {
    for ambiguous in [false, true] {
        let raw = if ambiguous {
            "```yaml\naction: tool\ntool: write\npath: must-not-exist.txt\ncontent: unsafe\n```\n```yaml\naction: finish\n```"
        } else {
            "action: tool\ntool: read"
        };
        let mut h = Harness::new(
            "  - id: blocker\n    action: pause",
            vec![Ok(raw), Ok(FINISH)],
        );
        let step = h.root.path().join("workflows/demo/step.md");
        fs::write(
            &step,
            fs::read_to_string(&step)
                .unwrap()
                .replace("tools: []", "tools: [read, write]"),
        )
        .unwrap();
        h.engine.tools.policies.write = "allow".into();
        h.run().await;
        h.assert_calls(2);
        assert_eq!(h.status(), "completed");
        assert_eq!(h.count("protocol.error"), 1);
        assert_eq!(h.count("tool.request"), 0);
        assert_eq!(h.count("step.error"), 0);
        assert!(!h.root.path().join("must-not-exist.txt").exists());
        let script = h.script.lock().unwrap();
        let correction = script.requests[1].last().unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(correction.contains(if ambiguous {
            "multiple fenced blocks"
        } else {
            "requires nonempty path"
        }));
    }
}
