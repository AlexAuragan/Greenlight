//! Read-only, branch-visible evidence snapshots. Export never runs verification.
use crate::{
    conversation_dump::Event,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Serialize)]
struct Artifact {
    event: i64,
    name: String,
    step: String,
    content: String,
}
#[derive(Serialize)]
struct Evidence {
    format: &'static str,
    session: Session,
    cutoff: Option<i64>,
    scope: &'static str,
    limitations: &'static str,
    records: Vec<Event>,
    artifacts: Vec<Artifact>,
    diagnostics: Vec<String>,
}
impl Store {
    pub fn verification_export(&self, selected: &Session) -> Result<Vec<u8>> {
        let tx = self.db.unchecked_transaction()?;
        let session = self.session(&selected.id)?;
        if session.active_branch != selected.active_branch {
            bail!("Branch changed; refresh before exporting evidence");
        }
        let history = self.history(&session)?;
        let cutoff = history.last().map(|e| e.0);
        let by_id: BTreeMap<_, _> = history.iter().map(|e| (e.0, e)).collect();
        let mut included = BTreeSet::new();
        let mut artifact_ids = BTreeSet::new();
        let mut diagnostics = Vec::new();
        let mut reference_count = 0usize;
        for (id, _, kind, raw) in &history {
            if kind.starts_with("verification.")
                || kind.starts_with("dependency.")
                || matches!(
                    kind.as_str(),
                    "step.definition"
                        | "step.started"
                        | "step.completed"
                        | "step.blocked"
                        | "step.deviation"
                        | "session.completed"
                        | "workspace.restored"
                        | "context.inputs"
                        | "human.approved"
                        | "human.declined"
                        | "file.changed"
                        | "artifact.user-edit"
                )
            {
                included.insert(*id);
            }
            if kind == "verification.evidence" {
                match serde_json::from_str::<Value>(raw) {
                    Ok(value) => {
                        for key in ["artifact_events", "observation_events"] {
                            if let Some(ids) = value[key].as_array() {
                                if ids.len()
                                    > self.limits.export_events.saturating_sub(reference_count)
                                {
                                    bail!("Evidence references exceed limits.export_events; no export published");
                                }
                                reference_count += ids.len();
                                for reference in ids {
                                    if let Some(reference) = reference.as_i64() {
                                        if key == "artifact_events" {
                                            artifact_ids.insert(reference);
                                        } else if by_id.get(&reference).is_some_and(|e| {
                                            matches!(
                                                e.2.as_str(),
                                                "tool.result"
                                                    | "tool.error"
                                                    | "verification.command"
                                                    | "verification.workspace"
                                                    | "verification.diagnostic"
                                                    | "verification.imported"
                                            )
                                        }) {
                                            included.insert(reference);
                                        } else {
                                            diagnostics.push(format!("Event #{id}: observation #{reference} missing, hidden or not a recorded tool/check outcome"));
                                        }
                                    } else {
                                        diagnostics
                                            .push(format!("Event #{id}: invalid {key} reference"));
                                    }
                                }
                            } else {
                                diagnostics
                                    .push(format!("Event #{id}: {key} references unavailable"));
                            }
                        }
                    }
                    Err(error) => diagnostics.push(format!(
                        "Malformed verification references at #{id}: {error}"
                    )),
                }
            }
            if matches!(kind.as_str(), "context.inputs" | "human.approved") {
                let manifest = if kind == "human.approved" {
                    serde_json::from_str::<crate::input_pins::Approval>(raw).map(|a| a.inputs)
                } else {
                    serde_json::from_str::<crate::input_pins::InputManifest>(raw)
                };
                match manifest {
                    Ok(manifest) => {
                        if manifest.artifacts.len() > self.limits.export_events.saturating_sub(reference_count) {
                            bail!("Input references exceed limits.export_events; no export published");
                        }
                        reference_count += manifest.artifacts.len();
                        artifact_ids.extend(manifest.artifacts.into_iter().map(|a|a.event_id));
                    }
                    Err(_) => diagnostics.push(format!("Input version references unavailable at event #{id} (legacy or malformed record)")),
                }
            }
        }
        // Include exact requests where durable correlation exists, never guess from adjacency.
        for (id, _, kind, raw) in &history {
            if kind == "tool.finished" {
                if let Ok(link) = serde_json::from_str::<crate::activity_cards::ToolFinished>(raw) {
                    if included.contains(&link.outcome_event) {
                        if by_id
                            .get(&link.request_event)
                            .is_some_and(|e| e.2 == "tool.request")
                        {
                            included.insert(link.request_event);
                            included.insert(*id);
                        } else {
                            diagnostics.push(format!(
                                "Tool request #{} unavailable for outcome #{}",
                                link.request_event, link.outcome_event
                            ));
                        }
                    }
                }
            }
        }
        let mut budget = crate::resource_limits::Budget::new(
            self.limits.materialized_bytes.min(self.limits.export_bytes),
        );
        let mut artifacts = Vec::new();
        for event in artifact_ids {
            if let Some(entry) = by_id.get(&event).filter(|e| e.2 == "artifact.created") {
                let name = &entry.3;
                included.insert(event);
                let versions = self.artifact_versions(&session, name)?;
                let version = versions
                    .iter()
                    .find(|v| v.event_id == event)
                    .context("Referenced artifact version metadata missing")?;
                // Missing/unreadable immutable blobs make this export fail, never appear empty.
                let content = self.read_blob_budget(&version.blob, &mut budget)?;
                artifacts.push(Artifact {
                    event,
                    name: name.clone(),
                    step: version.step.clone(),
                    content,
                });
            } else {
                diagnostics.push(format!("Artifact reference #{event} is missing or hidden by the selected branch; no body exported"));
            }
        }
        if included.len() > self.limits.export_events {
            bail!("Evidence exceeds limits.export_events; no export published");
        }
        let mut records = Vec::new();
        for id in included {
            let (_, step, kind, data) = by_id[&id];
            budget.charge(data.len() + step.len() + kind.len() + 128)?;
            let (branch, timestamp) = self.db.query_row(
                "SELECT branch,timestamp FROM events WHERE id=?1 AND session=?2",
                rusqlite::params![id, session.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            records.push(Event {
                id,
                branch,
                step: step.clone(),
                kind: kind.clone(),
                timestamp,
                data: data.clone(),
            });
        }
        if !records.iter().any(|e| e.kind.starts_with("verification.")) {
            diagnostics.push(
                "No verification records on this visible branch; this is not evidence of success"
                    .into(),
            );
        }
        let report=Evidence {
            format:"greenlight-verification-evidence-v1",session,cutoff,
            scope:"Selected session active branch and inherited history through fork cutoffs; all recorded attempts, not just the latest verdict",
            limitations:"Historical recorded evidence, not a fresh check or live filesystem attestation. verification.none is policy acceptance, not measured validation. Human decisions and model verdicts retain their recorded meaning. Tool outputs may already be truncated by the harness. Missing legacy references are explicit. Reports may contain project/tool output; review before sharing. Full provider requests, credentials and unreferenced artifacts are not collected. Raw check policies/outputs can themselves contain sensitive text.",
            records,artifacts,diagnostics,
        };
        let bytes = crate::resource_limits::json_bytes(&report, self.limits.export_bytes)?;
        tx.commit()?;
        Ok(bytes)
    }
    pub fn save_verification_export(&self, selected: &Session) -> Result<PathBuf> {
        let bytes = self.verification_export(selected)?;
        let parent = self.directory.parent().context("Missing state parent")?;
        let exports = crate::anchored_fs::Dir::open(parent)?.create_dirs(Path::new("exports"))?;
        let name = format!("verification-{}.json", uuid::Uuid::new_v4());
        exports.target(Path::new(&name))?.publish_new(&bytes)?;
        Ok(parent.join("exports").join(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn report_keeps_recorded_checks_and_exact_old_artifacts_without_provider_requests() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "verify").unwrap();
        store
            .add_artifact(&s, "work", "report.md", "reviewed original")
            .unwrap();
        let base = store.artifact_versions(&s, "report.md").unwrap()[0].event_id;
        store
            .add_artifact(&s, "work", "report.md", "later unreviewed")
            .unwrap();
        store
            .event(
                &s,
                Some("verify"),
                "model.request",
                "DO_NOT_EXPORT_PROVIDER_REQUEST",
            )
            .unwrap();
        store
            .event(
                &s,
                Some("verify"),
                "step.definition",
                "verification: command\nrun: cargo test",
            )
            .unwrap();
        let request = store
            .event(
                &s,
                Some("work"),
                "tool.request",
                "action: tool\ntool: shell\ninput: cargo test",
            )
            .unwrap();
        let output = store
            .tool_outcome(
                &s,
                "work",
                request,
                "shell",
                "tool.result",
                "exit_code: 1\nfailed assertion",
            )
            .unwrap();
        store.event(&s,Some("verify"),"verification.evidence",&json!({"attempt":"recorded-attempt","artifact_events":[base],"observation_events":[output]}).to_string()).unwrap();
        store
            .event(
                &s,
                Some("verify"),
                "verification.agent",
                "passed: false\nreason: failed assertion",
            )
            .unwrap();
        store
            .event(&s, Some("verify"), "verification.failed", "rejected")
            .unwrap();
        let before = store.history(&s).unwrap();
        let bytes = store.verification_export(&s).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let report: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(report["format"], "greenlight-verification-evidence-v1");
        assert_eq!(report["artifacts"][0]["event"], base);
        assert_eq!(report["artifacts"][0]["content"], "reviewed original");
        assert!(!text.contains("later unreviewed"));
        assert!(!text.contains("DO_NOT_EXPORT_PROVIDER_REQUEST"));
        assert!(report["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == request));
        assert!(report["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "verification.failed"));
        assert_eq!(store.history(&s).unwrap(), before);
    }
    #[test]
    fn branch_cutoffs_and_cross_session_references_never_export_hidden_bodies() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "verify").unwrap();
        let cutoff = store
            .event(&s, Some("verify"), "verification.none", "")
            .unwrap();
        store
            .add_artifact(&s, "verify", "hidden.md", "HIDDEN_FUTURE_BODY")
            .unwrap();
        let hidden = store.artifact_versions(&s, "hidden.md").unwrap()[0].event_id;
        store
            .event(&s, Some("verify"), "verification.passed", "")
            .unwrap();
        let other = store.create("demo", "other", "verify").unwrap();
        store
            .add_artifact(&other, "verify", "other.md", "OTHER_SESSION_BODY")
            .unwrap();
        let foreign = store.artifact_versions(&other, "other.md").unwrap()[0].event_id;
        let stale = s.clone();
        store.fork(&mut s, cutoff, "verify").unwrap();
        store
            .event(
                &s,
                Some("verify"),
                "verification.evidence",
                &json!({"artifact_events":[hidden,foreign],"observation_events":[hidden]})
                    .to_string(),
            )
            .unwrap();
        assert!(store.verification_export(&stale).is_err());
        let bytes = store.verification_export(&s).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let report: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(report["artifacts"].as_array().unwrap().is_empty());
        assert!(!text.contains("HIDDEN_FUTURE_BODY"));
        assert!(!text.contains("OTHER_SESSION_BODY"));
        assert!(!report["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "verification.passed"));
        assert_eq!(report["diagnostics"].as_array().unwrap().len(), 3);
    }
    #[test]
    fn unavailable_evidence_and_output_limits_publish_no_partial_success() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "verify").unwrap();
        store
            .add_artifact(&s, "work", "report.md", "required body")
            .unwrap();
        let version = store.artifact_versions(&s, "report.md").unwrap().remove(0);
        store
            .event(
                &s,
                Some("verify"),
                "verification.evidence",
                &json!({"artifact_events":[version.event_id],"observation_events":[]}).to_string(),
            )
            .unwrap();
        let exports = store.directory.parent().unwrap().join("exports");
        store.limits.export_events = 1;
        assert!(store.save_verification_export(&s).is_err());
        assert!(!exports.exists());
        store
            .event(
                &s,
                Some("verify"),
                "verification.evidence",
                &json!({
                    "artifact_events":[version.event_id,version.event_id], "observation_events":[]
                })
                .to_string(),
            )
            .unwrap();
        assert!(store
            .save_verification_export(&s)
            .unwrap_err()
            .to_string()
            .contains("references exceed"));
        store.limits.export_events = 100;
        std::fs::remove_file(store.directory.join("blobs").join(&version.blob)).unwrap();
        assert!(store.save_verification_export(&s).is_err());
        assert!(!exports.exists());
        let separate = tempfile::tempdir().unwrap();
        let mut small = Store::open(separate.path()).unwrap();
        let s = small.create("demo", "goal", "verify").unwrap();
        small
            .event(
                &s,
                Some("verify"),
                "verification.command",
                &"\0".repeat(1000),
            )
            .unwrap();
        small.limits.export_bytes = 5000;
        let error = small.save_verification_export(&s).unwrap_err();
        assert!(format!("{error:#}").contains("JSON exceeds"));
        assert!(!small.directory.parent().unwrap().join("exports").exists());
    }
    #[test]
    fn legacy_and_empty_histories_are_explicit_and_symlink_export_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "verify").unwrap();
        let empty: Value = serde_json::from_slice(&store.verification_export(&s).unwrap()).unwrap();
        assert!(empty["diagnostics"][0]
            .as_str()
            .unwrap()
            .contains("No verification records"));
        store
            .event(
                &s,
                Some("verify"),
                "verification.evidence",
                "malformed legacy references",
            )
            .unwrap();
        let legacy: Value =
            serde_json::from_slice(&store.verification_export(&s).unwrap()).unwrap();
        assert!(legacy["diagnostics"][0]
            .as_str()
            .unwrap()
            .contains("Malformed"));
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            store.directory.parent().unwrap().join("exports"),
        )
        .unwrap();
        assert!(store.save_verification_export(&s).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
