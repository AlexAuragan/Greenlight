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
                    .contains("read, write, shell, web and check are TOOL NAMES"));
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
        assert!(dump.markdown().unwrap().contains(if denied_by_host {
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

#[tokio::test]
async fn repeated_protocol_failure_pauses_without_dispatching_tools() {
    let mut h = Harness::new(PAUSE, vec![Ok("invalid"), Ok("invalid"), Ok("invalid")]);
    h.run().await;
    h.assert_calls(3);
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("protocol.repair-stopped"), 1);
    assert_eq!(h.count("tool.request"), 0);
}

#[tokio::test]
async fn conversation_budget_stops_before_a_provider_dispatch() {
    let mut h = Harness::new(PAUSE, vec![]);
    h.engine.cfg.session.max_conversation_bytes = 1;
    h.run().await;
    h.assert_calls(0);
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("context.limit-reached"), 1);
    assert_eq!(h.count("model.request"), 0);
}

#[tokio::test]
async fn previous_draft_is_reference_only_and_does_not_satisfy_new_outputs() {
    let mut h = Harness::new(PAUSE, vec![Ok(FINISH)]);
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: []\noutputs:\n  required: [plan.md]"),
    )
    .unwrap();
    let session = h.engine.store.session(&h.session).unwrap();
    h.engine
        .store
        .add_artifact(&session, "step", "plan.md", "OLD_DRAFT_EVIDENCE")
        .unwrap();
    h.run().await;
    assert_eq!(h.status(), "blocked");
    let script = h.script.lock().unwrap();
    let context = serde_json::to_string(&script.requests).unwrap();
    assert!(context.contains("OLD_DRAFT_EVIDENCE"));
    assert!(context.contains("does not satisfy this attempt"));
    assert!(context.contains("Required artifact outputs"));
    assert!(context.contains("Allowed deviation IDs"));
}

#[tokio::test]
async fn growing_conversation_stops_after_preserving_registered_artifact() {
    let large = format!(
        "action: artifact\nname: notes.md\ncontent: {}",
        "x".repeat(40_000)
    );
    let mut h = Harness::new(PAUSE, vec![Ok(&large)]);
    h.engine.cfg.session.max_conversation_bytes = 30_000;
    h.run().await;
    h.assert_calls(1);
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("context.limit-reached"), 1);
    let session = h.engine.store.session(&h.session).unwrap();
    assert_eq!(
        h.engine.store.artifacts(&session, "step").unwrap()[0]
            .content
            .len(),
        40_000
    );
}

#[tokio::test]
async fn user_rerun_preserves_failure_skips_approval_and_reaches_next_attempt() {
    let mut h = Harness::new(PAUSE, vec![Ok(FINISH)]);
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: [read]"),
    )
    .unwrap();
    fs::write(h.root.path().join("fixed.txt"), "USER_FIXED_ENVIRONMENT").unwrap();
    h.engine.tools.policies.read = "deny".into();
    let session = h.engine.store.session(&h.session).unwrap();
    let request = h
        .engine
        .store
        .event(
            &session,
            Some("step"),
            "tool.request",
            "action: tool\ntool: read\npath: fixed.txt",
        )
        .unwrap();
    let original = h
        .engine
        .store
        .tool_outcome(
            &session,
            "step",
            request,
            "read",
            "tool.error",
            "file missing before user fixed it",
        )
        .unwrap();
    h.engine
        .rerun_tool(&session.id, &session.active_branch, request)
        .await
        .unwrap();
    assert_eq!(h.status(), "blocked");
    assert!(h.script.lock().unwrap().requests.is_empty());
    let history = h.engine.store.history(&session).unwrap();
    assert!(history
        .iter()
        .any(|e| e.0 == original && e.3 == "file missing before user fixed it"));
    assert!(history
        .iter()
        .any(|e| e.2 == "protocol.approval" && e.3.contains("user-requested rerun")));
    let cards = crate::activity_cards::Projection::build(&history);
    assert!(cards
        .cards
        .values()
        .any(|card| card.title.starts_with("User rerun")));
    assert!(history
        .iter()
        .any(|e| e.2 == "tool.result" && e.3 == "USER_FIXED_ENVIRONMENT"));
    h.run().await;
    assert_eq!(h.status(), "completed");
    let requests = &h.script.lock().unwrap().requests;
    assert!(serde_json::to_string(requests)
        .unwrap()
        .contains("USER_FIXED_ENVIRONMENT"));
    assert_eq!(requests.len(), 1);
    // The authorization applies to this rerun, not subsequent model calls.
    assert!(h.engine.tools.permission("read", "fixed.txt").is_err());
}

#[tokio::test]
async fn user_rerun_writes_are_journaled_and_private_paths_remain_blocked() {
    let mut h = Harness::new(PAUSE, vec![]);
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: [write, read]"),
    )
    .unwrap();
    h.engine.tools.policies.write = "deny".into();
    let mut session = h.engine.store.session(&h.session).unwrap();
    session.status = "completed".into();
    h.engine.store.save(&session).unwrap();
    let request = h
        .engine
        .store
        .event(
            &session,
            Some("step"),
            "tool.request",
            "action: tool\ntool: write\npath: feature.txt\ncontent: revised",
        )
        .unwrap();
    h.engine
        .rerun_tool(&session.id, &session.active_branch, request)
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(h.root.path().join("feature.txt")).unwrap(),
        "revised"
    );
    assert_eq!(h.count("file.changed"), 1);
    assert_eq!(h.status(), "blocked");
    assert!(h.engine.tools.permission("write", "feature.txt").is_err());
    let request = h
        .engine
        .store
        .event(
            &session,
            Some("step"),
            "tool.request",
            "action: tool\ntool: read\npath: /etc/passwd",
        )
        .unwrap();
    h.engine
        .rerun_tool(&session.id, &session.active_branch, request)
        .await
        .unwrap();
    assert_eq!(h.count("tool.error"), 1);
    assert_eq!(h.count("tool.rerun-completed"), 2);
    assert!(h.script.lock().unwrap().requests.is_empty());
}

#[tokio::test]
async fn reruns_reject_live_owners_changed_branches_and_disabled_tools() {
    let mut h = Harness::new(PAUSE, vec![]);
    let session = h.engine.store.session(&h.session).unwrap();
    let request = h
        .engine
        .store
        .event(
            &session,
            Some("step"),
            "tool.request",
            "action: tool\ntool: read\npath: file.txt",
        )
        .unwrap();
    assert!(h
        .engine
        .rerun_tool(&session.id, "stale-branch", request)
        .await
        .is_err());
    assert_eq!(h.status(), "ready");
    let owner = h.engine.store.claim_execution(&session.id, None).unwrap();
    assert!(h
        .engine
        .rerun_tool(&session.id, &session.active_branch, request)
        .await
        .is_err());
    h.engine
        .store
        .finish_execution(&session.id, &owner, None)
        .unwrap();
    assert!(h
        .engine
        .rerun_tool(&session.id, &session.active_branch, request)
        .await
        .is_err());
    assert_eq!(h.count("tool.rerun"), 0);
    let other = h.engine.store.create("demo", "different", "step").unwrap();
    assert!(h.engine.store.rerun_request(&other, request).is_err());
    assert_eq!(
        h.engine.store.execution_state(&session.id).unwrap(),
        crate::session_execution::ExecutionState::Idle
    );
}

#[tokio::test]
async fn saved_coding_request_survives_reopen_without_repeating_tools_or_attempt() {
    let mut h = Harness::new(
        PAUSE,
        vec![
            Ok("action: tool\ntool: read\npath: source.txt"),
            Err("offline"),
            Ok(FINISH),
        ],
    );
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: [read]"),
    )
    .unwrap();
    fs::write(h.root.path().join("source.txt"), "original source").unwrap();
    h.run().await;
    let session = h.engine.store.session(&h.session).unwrap();
    let attempt = h.engine.store.latest_attempt(&session, "step").unwrap();
    let saved = h
        .engine
        .store
        .pending_continuation(&session, false)
        .unwrap()
        .unwrap();
    assert_eq!(saved.messages, h.script.lock().unwrap().requests[1]);
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    reopened.resume_saved(&session.id).await.unwrap();
    assert_eq!(
        reopened.store.session(&session.id).unwrap().status,
        "completed"
    );
    assert_eq!(
        reopened.store.latest_attempt(&session, "step").unwrap(),
        attempt
    );
    let history = reopened.store.history(&session).unwrap();
    assert_eq!(history.iter().filter(|e| e.2 == "tool.request").count(), 1);
    assert_eq!(history.iter().filter(|e| e.2 == "step.started").count(), 1);
    assert!(reopened
        .store
        .pending_continuation(&session, false)
        .unwrap()
        .is_none());
    let script = h.script.lock().unwrap();
    assert_eq!(script.requests[1], script.requests[2]);
}

#[tokio::test]
async fn saved_verifier_resumes_finish_without_repeating_coding_or_registered_artifacts() {
    let mut h = Harness::new(
        PAUSE,
        vec![
            Ok("action: artifact\nname: report.md\ncontent: findings"),
            Ok(FINISH),
            Err("offline"),
            Ok("passed: true\nreason: supported"),
        ],
    );
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("type: none", "type: agent\n  prompt: detector.md"),
    )
    .unwrap();
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    reopened.resume_saved(&s.id).await.unwrap();
    assert_eq!(reopened.store.session(&s.id).unwrap().status, "completed");
    assert_eq!(
        reopened
            .store
            .history(&s)
            .unwrap()
            .iter()
            .filter(|e| e.2 == "artifact.created")
            .count(),
        1
    );
    let script = h.script.lock().unwrap();
    assert_eq!(script.requests.len(), 4);
    assert_eq!(script.requests[2], script.requests[3]);
}

#[tokio::test]
async fn changed_workspace_rejects_saved_request_before_dispatch_and_keeps_checkpoint() {
    let mut h = Harness::new(PAUSE, vec![Err("offline")]);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    fs::write(h.root.path().join("source.txt"), "intervening change").unwrap();
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    assert!(reopened
        .resume_saved(&s.id)
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    assert!(reopened
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn credential_file_changes_do_not_invalidate_exact_request_checkpoint() {
    let mut h = Harness::new(PAUSE, vec![Err("offline"), Ok(FINISH)]);
    fs::write(
        h.root.path().join(".env"),
        "GREENLIGHT_TEST_UNUSED_KEY=old-key\n",
    )
    .unwrap();
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    fs::write(
        h.root.path().join(".env"),
        "GREENLIGHT_TEST_UNUSED_KEY=new-key\n",
    )
    .unwrap();
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    reopened.resume_saved(&s.id).await.unwrap();
    assert_eq!(h.script.lock().unwrap().requests.len(), 2);
}

#[tokio::test]
async fn saved_detector_resumes_without_counting_the_deviation_twice() {
    let mut h = Harness::new(PAUSE, vec![Ok(REPORT), Err("offline"), Ok(ACCEPT)]);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    reopened.resume_saved(&s.id).await.unwrap();
    let script = h.script.lock().unwrap();
    assert_eq!(script.requests.len(), 3);
    assert_eq!(script.requests[1], script.requests[2]);
    assert_eq!(
        reopened
            .store
            .history(&s)
            .unwrap()
            .iter()
            .filter(|e| e.2 == "step.deviation")
            .count(),
        1
    );
}

#[tokio::test]
async fn owner_recovery_is_required_but_does_not_discard_a_saved_request() {
    let mut h = Harness::new(PAUSE, vec![Err("offline"), Ok(FINISH)]);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    // Simulate a worker dying after claiming ownership; releasing the OS lock
    // does not erase its persistent execution record.
    let owner = h.engine.store.claim_execution(&s.id, None).unwrap();
    drop(owner);
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    assert!(reopened.resume_saved(&s.id).await.is_err());
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    reopened.store.recover_session(&s.id).unwrap();
    reopened.resume_saved(&s.id).await.unwrap();
    assert_eq!(h.script.lock().unwrap().requests.len(), 2);
}

#[tokio::test]
async fn saved_composite_verification_does_not_repeat_completed_human_checks() {
    let mut h = Harness::new(
        PAUSE,
        vec![
            Ok("action: finish\nactivity_title: Finishing evidence"),
            Err("offline"),
            Ok("passed: true\nreason: supported"),
        ],
    );
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file).unwrap().replace(
            "type: none",
            "type: all\n  checks:\n    - type: human\n    - type: agent\n      prompt: detector.md",
        ),
    )
    .unwrap();
    let (requests, _prompts) = std::sync::mpsc::channel();
    let (answers, responses) = std::sync::mpsc::channel();
    answers.send("y".into()).unwrap();
    h.engine.tools.use_tui(requests, responses);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let cp = h
        .engine
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .unwrap();
    assert_eq!(cp.checks, vec![Ok(())]);
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    reopened.resume_saved(&s.id).await.unwrap();
    assert_eq!(reopened.store.session(&s.id).unwrap().status, "completed");
    let history = reopened.store.history(&s).unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|e| e.2 == "verification.human")
            .count(),
        1
    );
    assert_eq!(
        history.iter().filter(|e| e.2 == "activity.title").count(),
        1
    );
    assert!(crate::activity_cards::Projection::build(&history)
        .malformed
        .is_empty());
    assert_eq!(h.script.lock().unwrap().requests.len(), 3);
}

#[tokio::test]
async fn corrupted_checkpoint_never_falls_back_to_fresh_execution() {
    let mut h = Harness::new(PAUSE, vec![Err("offline")]);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let history = h.engine.store.history(&s).unwrap();
    let reference = crate::continuation::pending_reference(&history, false)
        .unwrap()
        .unwrap();
    fs::write(
        h.engine
            .store
            .directory
            .join("blobs")
            .join(format!("trace-{reference}.json")),
        "corrupt",
    )
    .unwrap();
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    assert!(reopened.resume_saved(&s.id).await.is_err());
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    assert_eq!(reopened.store.session(&s.id).unwrap().status, "blocked");
}

#[tokio::test]
async fn saved_retry_after_survives_worker_cancellation_and_owner_recovery() {
    let mut h = Harness::new(PAUSE, vec![Ok(FINISH)]);
    let mut failure =
        crate::provider_failure::Failure::local(crate::provider_failure::Kind::RateLimit);
    failure.status = Some(429);
    failure.retry_after_seconds = Some(60);
    h.script
        .lock()
        .unwrap()
        .completions
        .push_back(crate::provider::Completion {
            content: Err("rate limited".into()),
            failure: Some(failure),
            raw_usage: None,
            response_id: None,
            response_model: None,
        });
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let cp = h
        .engine
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .unwrap();
    assert!(cp.cooldown_until.unwrap() > chrono::Utc::now());
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(30),
        reopened.resume_saved(&s.id)
    )
    .await
    .is_err());
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    reopened.store.recover_session(&s.id).unwrap();
    let mut elapsed = reopened
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .unwrap();
    elapsed.cooldown_until = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    reopened.store.save_continuation(&s, &elapsed).unwrap();
    reopened.resume_saved(&s.id).await.unwrap();
    assert_eq!(h.script.lock().unwrap().requests.len(), 2);
    assert_eq!(reopened.store.session(&s.id).unwrap().status, "completed");
}

#[tokio::test]
async fn saved_request_rechecks_files_after_its_cooldown_wait() {
    let mut h = Harness::new(PAUSE, vec![Err("offline"), Ok(FINISH)]);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let mut cp = h
        .engine
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .unwrap();
    cp.cooldown_until = Some(chrono::Utc::now() + chrono::Duration::milliseconds(300));
    h.engine.store.save_continuation(&s, &cp).unwrap();
    let root = h.root.path().to_path_buf();
    let id = s.id.clone();
    let changed = tokio::spawn(async move {
        let store = crate::store::Store::open(&root).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let session = store.session(&id).unwrap();
            if store
                .history(&session)
                .unwrap()
                .iter()
                .any(|e| e.2 == "continuation.retry-wait")
            {
                fs::write(root.join("changed-during-wait.txt"), "new evidence").unwrap();
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "saved retry never entered cooldown"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    });
    let mut reopened = Engine::open(h.root.path()).unwrap();
    reopened.provider.script = Some(h.script.clone());
    reopened.resume_saved(&s.id).await.unwrap();
    changed.await.unwrap();
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    assert_eq!(reopened.store.session(&s.id).unwrap().status, "blocked");
    assert!(reopened
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn manual_provider_retry_rejects_workspace_changes_while_the_prompt_waits() {
    let mut h = Harness::new(PAUSE, vec![Ok(FINISH)]);
    h.script.lock().unwrap().completions.push_back(
        crate::provider::Completion::parse(401, r#"{"error":{"message":"credentials rejected"}}"#)
            .unwrap(),
    );
    let (requests, prompts) = std::sync::mpsc::channel();
    let (answers, responses) = std::sync::mpsc::channel();
    h.engine.tools.use_tui(requests, responses);
    h.engine.interactive_provider_retry = true;
    let root = h.root.path().to_path_buf();
    let changed = std::thread::spawn(move || {
        let prompt = prompts
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert_eq!(prompt.mode, crate::tools::ApprovalMode::ProviderRetry);
        fs::write(root.join("changed-during-prompt.txt"), "new evidence").unwrap();
        answers.send("y".into()).unwrap();
    });
    h.run().await;
    changed.join().unwrap();
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    assert_eq!(h.status(), "blocked");
    assert_eq!(h.count("provider.retry-decision"), 1);
}

#[tokio::test]
async fn validation_rejects_a_stale_session_snapshot() {
    let mut h = Harness::new(PAUSE, vec![Err("offline")]);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    let cp = h
        .engine
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .unwrap();
    let mut current = s.clone();
    current.current_step = "different-step".into();
    h.engine.store.save(&current).unwrap();
    assert!(h
        .engine
        .validate_continuation(&s, &cp)
        .unwrap_err()
        .to_string()
        .contains("Session changed"));
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
}

#[tokio::test]
async fn automatic_provider_retry_rechecks_workspace_after_backoff() {
    let mut h = Harness::new(PAUSE, vec![Ok(FINISH)]);
    let mut failure =
        crate::provider::Completion::parse(429, r#"{"error":{"code":"rate_limit_exceeded"}}"#)
            .unwrap();
    failure.failure.as_mut().unwrap().retry_after_seconds = Some(1);
    h.script.lock().unwrap().completions.push_back(failure);
    let root = h.root.path().to_path_buf();
    let id = h.session.clone();
    let changed = std::thread::spawn(move || {
        let store = crate::store::Store::open(&root).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let session = store.session(&id).unwrap();
            if store
                .history(&session)
                .unwrap()
                .iter()
                .any(|e| e.2 == "provider.retry")
            {
                fs::write(root.join("changed-during-backoff.txt"), "new evidence").unwrap();
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    h.run().await;
    changed.join().unwrap();
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    assert_eq!(h.status(), "blocked");
}

#[test]
fn continuation_restart_child() {
    let Some(root) = std::env::var_os("GREENLIGHT_CONTINUATION_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    crate::project::load_environment(&root).unwrap();
    let mut engine = Engine::open(&root).unwrap();
    assert_eq!(
        engine.provider.connection_info().credential.masked,
        "fresh…last5"
    );
    let id = std::env::var("GREENLIGHT_CONTINUATION_TEST_SESSION").unwrap();
    let session = engine.store.session(&id).unwrap();
    let checkpoint = engine
        .store
        .pending_continuation(&session, false)
        .unwrap()
        .unwrap();
    let script = Arc::new(Mutex::new(Script {
        responses: vec![Ok(FINISH.into())].into(),
        ..Default::default()
    }));
    engine.provider.script = Some(script.clone());
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(engine.resume_saved(&id))
        .unwrap();
    assert_eq!(script.lock().unwrap().requests, vec![checkpoint.messages]);
    assert_eq!(engine.store.session(&id).unwrap().status, "completed");
}

#[tokio::test]
async fn real_process_restart_loads_updated_credentials_without_repeating_tools() {
    let mut h = Harness::new(
        PAUSE,
        vec![
            Ok("action: tool\ntool: read\npath: source.txt"),
            Err("offline"),
        ],
    );
    let file = h.root.path().join("workflows/demo/step.md");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("tools: []", "tools: [read]"),
    )
    .unwrap();
    fs::write(h.root.path().join("source.txt"), "existing source").unwrap();
    h.run().await;
    let session = h.engine.store.session(&h.session).unwrap();
    let attempt = h.engine.store.latest_attempt(&session, "step").unwrap();
    fs::write(
        h.root.path().join(".env"),
        "GREENLIGHT_TEST_UNUSED_KEY=fresh-private-key-last5\n",
    )
    .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "deviation_tests::continuation_restart_child",
            "--nocapture",
        ])
        .env("GREENLIGHT_CONTINUATION_TEST_ROOT", h.root.path())
        .env("GREENLIGHT_CONTINUATION_TEST_SESSION", &h.session)
        .env_remove("GREENLIGHT_TEST_UNUSED_KEY")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "child stdout: {}\nchild stderr: {}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(h.status(), "completed");
    assert_eq!(h.count("tool.request"), 1);
    assert_eq!(
        h.engine.store.latest_attempt(&session, "step").unwrap(),
        attempt
    );
}

#[tokio::test]
async fn budget_pause_can_resume_exact_saved_messages_after_only_the_limit_changes() {
    let mut h = Harness::new(PAUSE, vec![Ok(FINISH)]);
    h.engine.cfg.budgets.session.requests = Some(0);
    h.run().await;
    let s = h.engine.store.session(&h.session).unwrap();
    assert_eq!(s.status, "blocked");
    assert!(h.script.lock().unwrap().requests.is_empty());
    let cp = h
        .engine
        .store
        .pending_continuation(&s, false)
        .unwrap()
        .unwrap();
    let attempt = cp.attempt.clone();
    h.engine.cfg.budgets.session.requests = Some(1);
    h.engine.resume_saved(&s.id).await.unwrap();
    assert_eq!(h.engine.store.session(&s.id).unwrap().status, "completed");
    assert_eq!(h.script.lock().unwrap().requests.len(), 1);
    assert_eq!(h.script.lock().unwrap().requests[0], cp.messages);
    let calls = h
        .engine
        .store
        .usage_report(&s, crate::usage::Scope::Session)
        .unwrap()
        .calls;
    assert_eq!(calls[0].attempt, attempt);
}
