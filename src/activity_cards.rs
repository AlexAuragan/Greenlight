//! Recorded tool correlations and read-only semantic card projections. Titles describe harness records,
//! never inferred execution or model-authored claims.
use crate::{
    store::{Session, Store},
    tools::AgentAction,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

type Event = (i64, String, String, String);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolFinished {
    pub request_event: i64,
    pub outcome_event: i64,
    pub tool: String,
    pub exit_code: Option<i32>,
}
fn shell_exit(body: &str) -> Option<i32> {
    body.lines()
        .next()?
        .strip_prefix("exit_code: ")?
        .parse()
        .ok()
}
impl Store {
    /// Publish output and correlation together, after tool execution has returned.
    pub fn tool_outcome(
        &self,
        s: &Session,
        step: &str,
        request: i64,
        tool: &str,
        kind: &str,
        body: &str,
    ) -> Result<i64> {
        if !matches!(kind, "tool.result" | "tool.error") {
            bail!("invalid tool outcome event kind")
        }
        let tx = self
            .db
            .unchecked_transaction()
            .context("Cannot record tool outcome; the tool may already have run")?;
        let outcome = self
            .event(s, Some(step), kind, body)
            .context("Cannot publish tool outcome; inspect recorded effects before resuming")?;
        let metadata = ToolFinished {
            request_event: request,
            outcome_event: outcome,
            tool: tool.into(),
            exit_code: if tool == "shell" && kind == "tool.result" {
                shell_exit(body)
            } else {
                None
            },
        };
        self.event(
            s,
            Some(step),
            "tool.finished",
            &serde_json::to_string(&metadata)?,
        )
        .context("Cannot publish tool correlation; the tool may already have run")?;
        tx.commit()
            .context("Cannot commit tool outcome; inspect recorded effects before resuming")?;
        Ok(outcome)
    }
}
#[derive(Clone, Debug)]
pub struct ToolEvidence {
    pub metadata: ToolFinished,
    pub request: String,
    pub target: String,
}
#[derive(Default)]
pub struct Projection {
    pub cards: HashMap<i64, Card>,
    pub tools: HashMap<i64, ToolEvidence>,
    pub malformed: HashMap<i64, String>,
}
impl Projection {
    pub fn build(history: &[Event]) -> Self {
        let records: HashMap<_, _> = history.iter().map(|e| (e.0, e)).collect();
        let mut boundaries = history
            .iter()
            .filter(|e| matches!(e.2.as_str(), "step.started" | "branch.forked"))
            .map(|e| e.0)
            .collect::<Vec<_>>();
        boundaries.sort_unstable();
        let mut result = Self::default();
        let mut seen_requests = std::collections::HashSet::new();
        for (id, step, kind, raw) in history {
            if kind != "tool.finished" {
                continue;
            }
            let evidence = (|| -> Result<ToolEvidence> {
                let metadata: ToolFinished = serde_json::from_str(raw)?;
                let request = records
                    .get(&metadata.request_event)
                    .context("request event is not branch-visible")?;
                let outcome = records
                    .get(&metadata.outcome_event)
                    .context("outcome event is not branch-visible")?;
                if request.2 != "tool.request"
                    || !matches!(outcome.2.as_str(), "tool.result" | "tool.error")
                    || request.1 != *step
                    || outcome.1 != *step
                    || !(request.0 < outcome.0 && outcome.0 < *id)
                {
                    bail!("tool references have incompatible type, step or order")
                }
                let boundary = boundaries.partition_point(|boundary| *boundary <= request.0);
                if boundaries
                    .get(boundary)
                    .is_some_and(|boundary| *boundary < *id)
                {
                    bail!("tool references cross an attempt or branch boundary")
                }
                if result.tools.contains_key(&outcome.0) {
                    bail!("duplicate correlation for one outcome")
                }
                if seen_requests.contains(&metadata.request_event) {
                    bail!("duplicate outcome for one request")
                }
                let action: AgentAction = serde_yaml::from_str(&request.3)?;
                if action.action != "tool" || action.tool.as_deref() != Some(metadata.tool.as_str())
                {
                    bail!("tool metadata disagrees with recorded request")
                }
                let expected = if metadata.tool == "shell" && outcome.2 == "tool.result" {
                    shell_exit(&outcome.3)
                } else {
                    None
                };
                if metadata.exit_code != expected {
                    bail!("exit code disagrees with recorded shell result")
                }
                let target = match metadata.tool.as_str() {
                    "shell" | "web" => action.input,
                    "read" | "write" => action.path,
                    _ => action.path.or(action.input),
                }
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "No target supplied".into());
                Ok(ToolEvidence {
                    metadata,
                    request: request.3.clone(),
                    target,
                })
            })();
            match evidence {
                Ok(evidence) => {
                    seen_requests.insert(evidence.metadata.request_event);
                    result
                        .tools
                        .insert(evidence.metadata.outcome_event, evidence);
                }
                Err(error) => {
                    result
                        .malformed
                        .insert(*id, format!("Invalid tool correlation #{id}: {error:#}"));
                }
            }
        }
        for (id, _, kind, raw) in history {
            if !is_protocol(kind) {
                let card = describe(*id, kind, raw, result.tools.get(id));
                result.cards.insert(*id, card);
            }
        }
        result
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Good,
    Attention,
    Error,
}
#[derive(Clone, Debug)]
pub struct Card {
    pub title: String,
    pub summary: String,
    pub tone: Tone,
}
fn short(raw: &str) -> String {
    raw.lines()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}
fn card(title: impl Into<String>, summary: impl Into<String>, tone: Tone) -> Card {
    Card {
        title: title.into(),
        summary: summary.into(),
        tone,
    }
}
pub fn is_protocol(kind: &str) -> bool {
    matches!(
        kind,
        "step.definition"
            | "context.assembled"
            | "model.output"
            | "tool.request"
            | "tool.finished"
            | "permission.decision"
            | "model.call-started"
            | "model.usage"
            | "workspace.manifest.before"
            | "workspace.manifest.after"
    )
}
pub fn describe(id: i64, kind: &str, raw: &str, tool: Option<&ToolEvidence>) -> Card {
    if let Some(tool) = tool {
        let (status, tone) = if kind == "tool.error" {
            ("error".into(), Tone::Error)
        } else if let Some(exit) = tool.metadata.exit_code {
            (
                format!("exit {exit}"),
                if exit == 0 { Tone::Good } else { Tone::Error },
            )
        } else {
            ("returned".into(), Tone::Info)
        };
        return card(
            format!("Tool · {} · {status}", short(&tool.metadata.tool)),
            format!(
                "{} · request #{} → output #{id}",
                short(&tool.target),
                tool.metadata.request_event
            ),
            tone,
        );
    }
    match kind {
        "tool.result" => card("Tool output · unlinked", short(raw), Tone::Info),
        "tool.error" => card("Tool error · unlinked", short(raw), Tone::Error),
        "verification.passed" => card(
            "Verification passed",
            "Recorded aggregate policy verdict",
            Tone::Good,
        ),
        "verification.failed" => card("Verification check failed", short(raw), Tone::Error),
        "verification.none" => card("Verification policy", "No checks required", Tone::Info),
        "verification.command" => match shell_exit(raw) {
            Some(exit) => card(
                format!("Verification command · exit {exit}"),
                "Expand for recorded output",
                if exit == 0 { Tone::Good } else { Tone::Error },
            ),
            None => card(
                "Verification command",
                "Exit code unavailable; inspect recorded output",
                Tone::Attention,
            ),
        },
        "verification.agent" => {
            let report = raw
                .trim()
                .trim_start_matches("```yaml")
                .trim_start_matches("```yml")
                .trim_end_matches("```")
                .trim();
            #[derive(Deserialize)]
            struct ReviewSummary {
                passed: bool,
            }
            let passed = if report.len() <= 8192 {
                serde_yaml::from_str::<ReviewSummary>(report)
                    .ok()
                    .map(|v| v.passed)
            } else {
                None
            };
            card(
                "Agent review report",
                match passed {
                    Some(true) => "Model reports pass; see aggregate verdict separately",
                    Some(false) => "Model reports rejection; see aggregate verdict separately",
                    None => "Unparsed report; expand raw evidence",
                },
                Tone::Info,
            )
        }
        "verification.evidence" => {
            let v = serde_json::from_str::<serde_json::Value>(raw).ok();
            let summary = v
                .as_ref()
                .and_then(|v| {
                    Some(format!(
                        "{} artifact refs · {} observation refs",
                        v["artifact_events"].as_array()?.len(),
                        v["observation_events"].as_array()?.len()
                    ))
                })
                .unwrap_or_else(|| "Unparsed evidence manifest".into());
            card("Verification evidence", summary, Tone::Info)
        }
        "file.changed" => card(
            format!("File changed · {}", short(raw)),
            "Recorded native write · d opens journal diff",
            Tone::Info,
        ),
        "workspace.restored" => card(
            format!("File restored · {}", short(raw)),
            "Recorded restore · d opens journal diff",
            Tone::Info,
        ),
        "artifact.created" => card(
            format!("Artifact stored · {}", short(raw)),
            format!("Version #{id} · o opens this exact version"),
            Tone::Info,
        ),
        "provider.failure" => {
            let parsed = serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| {
                    serde_json::from_value::<crate::provider_failure::Failure>(v["failure"].clone())
                        .ok()
                });
            match parsed {
                Some(f) => card(
                    format!("Provider · {:?}", f.kind),
                    f.to_string(),
                    if f.retryable {
                        Tone::Attention
                    } else {
                        Tone::Error
                    },
                ),
                None => card(
                    "Provider failure",
                    "Unparsed failure record; inspect evidence",
                    Tone::Error,
                ),
            }
        }
        "provider.retry" => {
            let v = serde_json::from_str::<serde_json::Value>(raw).ok();
            let summary = v
                .and_then(|v| {
                    Some(format!(
                        "Request {} · wait {} ms · x cancels",
                        v["next_request"].as_u64()?,
                        v["wait_ms"].as_u64()?
                    ))
                })
                .unwrap_or_else(|| "Inspect recorded retry schedule".into());
            card("Provider retry scheduled", summary, Tone::Attention)
        }
        "provider.retry-stopped" | "provider.retry-cancelled" => {
            card("Provider retry stopped", short(raw), Tone::Attention)
        }
        "workspace.restore-failed" => {
            let summary = serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| {
                    Some(format!(
                        "{} files changed · {} journaled; inspect before retry",
                        v["changed_files"].as_u64()?,
                        v["journaled_files"].as_u64()?
                    ))
                })
                .unwrap_or_else(|| "Unparsed restore failure; inspect evidence".into());
            card("Restore incomplete", summary, Tone::Error)
        }
        "event.invalid" => card("Malformed record", short(raw), Tone::Error),
        "step.error" | "runner.error" => card("Execution error", short(raw), Tone::Error),
        "step.blocked" => card("Step blocked", short(raw), Tone::Attention),
        "step.completed" => card("Step completed", short(raw), Tone::Good),
        "step.started" => card("Step started", short(raw), Tone::Info),
        "session.started" => card("Session started", short(raw), Tone::Info),
        "session.completed" => card("Session completed", short(raw), Tone::Good),
        "user.instruction" => card("You instructed", short(raw), Tone::Info),
        "step.deviation" => card("Deviation recorded", short(raw), Tone::Attention),
        "branch.forked" => card("Branch forked", short(raw), Tone::Info),
        "runner.recovered" => card("Execution owner recovered", short(raw), Tone::Attention),
        "human.approved" => card(
            "Human review recorded",
            "Exact input references available in evidence",
            Tone::Info,
        ),
        _ => card(kind, short(raw), Tone::Info),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(store: &Store, s: &Session, tool: &str) -> i64 {
        store
            .event(
                s,
                Some("work"),
                "tool.request",
                &format!("action: tool\ntool: {tool}\npath: notes.md\ninput: echo hi\n"),
            )
            .unwrap()
    }
    #[test]
    fn linked_shell_status_is_recorded_but_read_contents_never_become_a_verdict() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let req = request(&store, &s, "shell");
        let shell = store
            .tool_outcome(
                &s,
                "work",
                req,
                "shell",
                "tool.result",
                "exit_code: 1\nall tests passed",
            )
            .unwrap();
        let req = request(&store, &s, "read");
        let read = store
            .tool_outcome(
                &s,
                "work",
                req,
                "read",
                "tool.result",
                "exit_code: 0\nall tests passed",
            )
            .unwrap();
        let projection = Projection::build(&store.history(&s).unwrap());
        assert!(projection.malformed.is_empty());
        assert_eq!(projection.tools.len(), 2);
        assert_eq!(projection.tools[&shell].target, "echo hi");
        assert_eq!(projection.tools[&read].target, "notes.md");
        let card = describe(
            shell,
            "tool.result",
            "exit_code: 1",
            projection.tools.get(&shell),
        );
        assert_eq!(card.tone, Tone::Error);
        assert!(card.title.contains("exit 1"));
        let card = describe(
            read,
            "tool.result",
            "exit_code: 0",
            projection.tools.get(&read),
        );
        assert_eq!(card.tone, Tone::Info);
        assert!(card.title.contains("returned"));
        assert!(!card.title.contains("exit"));
        assert_eq!(
            describe(
                0,
                "verification.agent",
                "passed: true\nreason: all good",
                None
            )
            .tone,
            Tone::Info
        );
        assert!(describe(0, "verification.failed", "rejected", None)
            .title
            .contains("check failed"));
        assert_eq!(
            describe(0, "verification.passed", "", None).tone,
            Tone::Good
        );
    }
    #[test]
    fn correlation_cannot_cross_branch_cutoffs_or_attempts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "work").unwrap();
        let req = request(&store, &s, "read");
        let outcome = store
            .tool_outcome(&s, "work", req, "read", "tool.result", "body")
            .unwrap();
        store.fork(&mut s, req, "work").unwrap();
        let projection = Projection::build(&store.history(&s).unwrap());
        assert!(projection.tools.is_empty());
        let metadata = ToolFinished {
            request_event: req,
            outcome_event: outcome,
            tool: "read".into(),
            exit_code: None,
        };
        let hidden = store
            .event(
                &s,
                Some("work"),
                "tool.finished",
                &serde_json::to_string(&metadata).unwrap(),
            )
            .unwrap();
        let projection = Projection::build(&store.history(&s).unwrap());
        assert!(projection.malformed.contains_key(&hidden));
        let req = request(&store, &s, "read");
        store
            .begin_attempt(&s, "work", "id: work", "fixture")
            .unwrap();
        store
            .tool_outcome(&s, "work", req, "read", "tool.result", "body")
            .unwrap();
        let projection = Projection::build(&store.history(&s).unwrap());
        assert!(projection.tools.is_empty());
        assert!(projection
            .malformed
            .values()
            .any(|s| s.contains("boundary")));
    }
    #[test]
    fn malformed_duplicate_and_forged_exit_metadata_stay_visible_as_errors() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let req = request(&store, &s, "shell");
        let outcome = store
            .tool_outcome(
                &s,
                "work",
                req,
                "shell",
                "tool.result",
                "exit_code: 1\nfailed",
            )
            .unwrap();
        let metadata = ToolFinished {
            request_event: req,
            outcome_event: outcome,
            tool: "shell".into(),
            exit_code: Some(1),
        };
        let duplicate = store
            .event(
                &s,
                Some("work"),
                "tool.finished",
                &serde_json::to_string(&metadata).unwrap(),
            )
            .unwrap();
        let bad = store
            .event(&s, Some("work"), "tool.finished", "malformed")
            .unwrap();
        let projection = Projection::build(&store.history(&s).unwrap());
        assert_eq!(projection.tools.len(), 1);
        assert!(projection.malformed.contains_key(&duplicate));
        assert!(projection.malformed.contains_key(&bad));
        let mut history = store.history(&s).unwrap();
        history.retain(|e| e.0 != duplicate && e.0 != bad);
        let meta = history.iter_mut().find(|e| e.2 == "tool.finished").unwrap();
        let forged = ToolFinished {
            exit_code: Some(0),
            ..metadata
        };
        meta.3 = serde_json::to_string(&forged).unwrap();
        let projection = Projection::build(&history);
        assert!(projection.tools.is_empty());
        assert!(projection
            .malformed
            .values()
            .any(|s| s.contains("exit code")));
    }
    #[test]
    fn output_and_correlation_publication_are_atomic() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let req = request(&store, &s, "read");
        store.db.execute_batch("CREATE TRIGGER reject_tool_link BEFORE INSERT ON events WHEN NEW.kind='tool.finished' BEGIN SELECT RAISE(ABORT,'injected correlation failure'); END;").unwrap();
        assert!(store
            .tool_outcome(&s, "work", req, "read", "tool.result", "body")
            .is_err());
        assert!(!store
            .history(&s)
            .unwrap()
            .iter()
            .any(|e| e.2 == "tool.result"));
        store
            .db
            .execute_batch("DROP TRIGGER reject_tool_link")
            .unwrap();
        store
            .tool_outcome(&s, "work", req, "read", "tool.result", "body")
            .unwrap();
        assert_eq!(
            Projection::build(&store.history(&s).unwrap()).tools.len(),
            1
        );
    }
    #[test]
    fn one_request_cannot_be_assigned_two_outcomes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let req = request(&store, &s, "read");
        store
            .tool_outcome(&s, "work", req, "read", "tool.result", "body")
            .unwrap();
        store
            .tool_outcome(&s, "work", req, "read", "tool.error", "second result")
            .unwrap();
        let projection = Projection::build(&store.history(&s).unwrap());
        assert_eq!(projection.tools.len(), 1);
        assert!(projection
            .malformed
            .values()
            .any(|error| error.contains("for one request")));
    }
    #[test]
    fn legacy_unknown_and_structured_failures_keep_conservative_summaries() {
        assert_eq!(
            describe(1, "tool.result", "all checks passed", None).tone,
            Tone::Info
        );
        assert!(describe(1, "tool.result", "all checks passed", None)
            .title
            .contains("unlinked"));
        assert_eq!(
            describe(1, "future.event", "success", None).title,
            "future.event"
        );
        let restore = describe(
            1,
            "workspace.restore-failed",
            r#"{"changed_files":2,"journaled_files":1,"error":"partial"}"#,
            None,
        );
        assert_eq!(restore.tone, Tone::Error);
        assert!(restore.summary.contains("2 files changed · 1 journaled"));
        let failure =
            crate::provider_failure::Failure::local(crate::provider_failure::Kind::Credentials);
        let card = describe(
            1,
            "provider.failure",
            &serde_json::json!({"failure":failure}).to_string(),
            None,
        );
        assert!(card.summary.contains("credentials"));
        assert_eq!(card.tone, Tone::Error);
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::{engine::Engine, provider::Script};
    use std::{
        fs,
        sync::{Arc, Mutex},
    };
    #[tokio::test]
    async fn real_engine_execution_records_exact_request_output_links() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: fixture\n",
        )
        .unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(
            folder.join("work.md"),
            "---\nid: work\ntools: [read]\nverification:\n  type: none\n---\nWork\n",
        )
        .unwrap();
        fs::write(root.path().join("notes.md"), "recorded file body").unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        engine.provider.script = Some(Arc::new(Mutex::new(Script {
            responses: vec![
                Ok("action: tool\ntool: read\npath: notes.md".into()),
                Ok("action: finish".into()),
            ]
            .into(),
            ..Default::default()
        })));
        let s = engine.store.create("demo", "Goal", "work").unwrap();
        engine.run(&s.id, None, None).await.unwrap();
        let history = engine.store.history(&s).unwrap();
        let projection = Projection::build(&history);
        assert!(projection.malformed.is_empty());
        assert_eq!(projection.tools.len(), 1);
        let (id, tool) = projection.tools.iter().next().unwrap();
        assert_eq!(tool.metadata.tool, "read");
        assert!(tool.request.contains("notes.md"));
        let output = history.iter().find(|event| event.0 == *id).unwrap();
        assert_eq!(output.2, "tool.result");
        assert_eq!(output.3, "recorded file body");
        assert!(projection.cards[id].title.contains("returned"));
        assert!(projection
            .cards
            .values()
            .any(|card| card.title == "Verification passed"));
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::engine::Engine;
    use std::fs;
    #[tokio::test]
    async fn denied_command_step_records_request_and_error_without_running_the_command() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: fixture\npermissions:\n  shell: deny\n",
        )
        .unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(folder.join("work.md"),"---\nid: work\nkind: command\ncommand: touch should-not-exist\nverification:\n  type: none\n---\nWork\n").unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        let s = engine.store.create("demo", "Goal", "work").unwrap();
        engine.run(&s.id, None, None).await.unwrap();
        let history = engine.store.history(&s).unwrap();
        let projection = Projection::build(&history);
        assert!(projection.malformed.is_empty());
        assert_eq!(projection.tools.len(), 1);
        let (id, evidence) = projection.tools.iter().next().unwrap();
        assert_eq!(evidence.metadata.tool, "shell");
        assert_eq!(evidence.target, "touch should-not-exist");
        assert_eq!(projection.cards[id].tone, Tone::Error);
        assert!(history
            .iter()
            .any(|event| event.0 == *id && event.2 == "tool.error" && event.3.contains("policy")));
        assert!(!root.path().join("should-not-exist").exists());
        assert_eq!(engine.store.session(&s.id).unwrap().status, "blocked");
    }
}
