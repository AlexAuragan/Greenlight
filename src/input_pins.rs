//! Immutable input references and human approval provenance, scoped by branch.
use crate::store::{Artifact, Session, Store};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct InputVersion {
    name: String,
    event_id: i64,
}

#[derive(Serialize, Deserialize)]
struct InputManifest {
    attempt: String,
    artifacts: Vec<InputVersion>,
    #[serde(default)]
    approval_events: Vec<i64>,
}

#[derive(Serialize, Deserialize)]
struct Approval {
    input_manifest: i64,
    inputs: InputManifest,
}

pub(crate) fn validate_recorded_event(kind: &str, raw: &str) -> Result<()> {
    if kind=="human.approved" {
        if !raw.is_empty() { serde_json::from_str::<Approval>(raw)?; }
    } else { serde_json::from_str::<InputManifest>(raw)?; }
    Ok(())
}

impl Store {
    fn input_manifest(
        &self,
        s: &Session,
        step: &str,
        inputs: &[Artifact],
    ) -> Result<InputManifest> {
        Ok(InputManifest {
            attempt: self.active_attempt(s, step)?.0,
            approval_events: Vec::new(),
            artifacts: inputs
                .iter()
                .map(|a| InputVersion {
                    name: a.name.clone(),
                    event_id: a.event_id,
                })
                .collect(),
        })
    }

    #[cfg(test)]
    pub fn record_input_manifest(
        &self,
        s: &Session,
        step: &str,
        inputs: &[Artifact],
    ) -> Result<i64> {
        self.record_approved_input_manifest(s, step, inputs, Vec::new())
    }

    pub fn record_approved_input_manifest(
        &self,
        s: &Session,
        step: &str,
        inputs: &[Artifact],
        approvals: Vec<i64>,
    ) -> Result<i64> {
        let mut manifest = self.input_manifest(s, step, inputs)?;
        manifest.approval_events = approvals;
        self.event(
            s,
            Some(step),
            "context.inputs",
            &serde_json::to_string(&manifest)?,
        )
    }

    pub fn approve_inputs<F>(
        &self,
        s: &Session,
        step: &str,
        manifest: i64,
        inputs: &[Artifact],
        resolve: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Result<Vec<Artifact>>,
    {
        let tx = self.db.unchecked_transaction()?;
        // A response approves the displayed snapshot, never an unseen update.
        let latest = resolve()?;
        if latest.len() != inputs.len() {
            bail!("approval inputs changed while awaiting a decision; rerun human step {step}");
        }
        for input in inputs {
            if !latest
                .iter()
                .any(|a| a.name == input.name && a.event_id == input.event_id)
            {
                bail!("approval input {} changed while awaiting a decision; rerun human step {step} to review and approve the new version", input.name);
            }
        }
        let approval = Approval {
            input_manifest: manifest,
            inputs: self.input_manifest(s, step, inputs)?,
        };
        self.event(
            s,
            Some(step),
            "human.approved",
            &serde_json::to_string(&approval)?,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn gate_input_names(&self, s: &Session, gate: &str) -> Result<Vec<String>> {
        let history = self.history(s)?;
        let Some((_, _, _, raw)) = history
            .iter()
            .rev()
            .find(|(_, step, kind, _)| step == gate && kind == "context.inputs")
        else {
            return Ok(Vec::new());
        };
        let manifest: InputManifest =
            serde_json::from_str(raw).context("invalid gate input manifest")?;
        Ok(manifest.artifacts.into_iter().map(|a| a.name).collect())
    }

    pub fn require_approved_input(&self, s: &Session, gate: &str, input: &Artifact) -> Result<i64> {
        let history = self.history(s)?;
        let start = history
            .iter()
            .rev()
            .find(|(_, step, kind, _)| step == gate && kind == "step.started")
            .with_context(|| {
                format!(
                    "input {} requires human approval at step {gate}",
                    input.name
                )
            })?
            .0;
        let approved = history
            .iter()
            .rev()
            .find(|(id, step, kind, _)| *id > start && step == gate && kind == "human.approved")
            .with_context(|| {
                format!(
                    "input {} requires reapproval at human step {gate}",
                    input.name
                )
            })?;
        if !history
            .iter()
            .any(|(id, step, kind, _)| *id > approved.0 && step == gate && kind == "step.completed")
        {
            bail!(
                "human step {gate} has not completed; reapprove {} before continuing",
                input.name
            );
        }
        let approval: Approval = serde_json::from_str(&approved.3).with_context(|| {
            format!(
                "legacy or invalid approval at step {gate}; reapprove {}",
                input.name
            )
        })?;
        if !approval
            .inputs
            .artifacts
            .iter()
            .any(|a| a.name == input.name && a.event_id == input.event_id)
        {
            bail!("input {} changed since approval; reapprove it at human step {gate} before continuing",input.name);
        }
        Ok(approved.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approve(store: &Store, session: &Session) -> Artifact {
        store
            .begin_attempt(session, "approve", "gate definition", "fixture")
            .unwrap();
        let inputs = store.artifacts(session, "session").unwrap();
        let manifest = store
            .record_input_manifest(session, "approve", &inputs)
            .unwrap();
        store
            .approve_inputs(session, "approve", manifest, &inputs, || {
                store.artifacts(session, "session")
            })
            .unwrap();
        store
            .event(session, Some("approve"), "step.completed", "approved")
            .unwrap();
        inputs.into_iter().find(|a| a.name == "plan.md").unwrap()
    }

    #[test]
    fn edited_inputs_need_reapproval_and_retries_cannot_reuse_an_old_decision() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "approve").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "original")
            .unwrap();
        let old = approve(&store, &session);
        store
            .require_approved_input(&session, "approve", &old)
            .unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "edited")
            .unwrap();
        let edited = store.artifacts(&session, "session").unwrap().remove(0);
        assert!(store
            .require_approved_input(&session, "approve", &edited)
            .is_err());
        let approved = approve(&store, &session);
        store
            .require_approved_input(&session, "approve", &approved)
            .unwrap();
        store
            .begin_attempt(&session, "approve", "retry gate", "fixture")
            .unwrap();
        store
            .event(&session, Some("approve"), "human.declined", "")
            .unwrap();
        store
            .event(&session, Some("approve"), "step.blocked", "declined")
            .unwrap();
        assert!(store
            .require_approved_input(&session, "approve", &approved)
            .is_err());
    }

    #[test]
    fn approval_does_not_accept_an_edit_while_the_prompt_is_pending() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "approve").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "displayed")
            .unwrap();
        store
            .begin_attempt(&session, "approve", "definition", "fixture")
            .unwrap();
        let shown = store.artifacts(&session, "session").unwrap();
        let manifest = store
            .record_input_manifest(&session, "approve", &shown)
            .unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "unseen edit")
            .unwrap();
        assert!(store
            .approve_inputs(&session, "approve", manifest, &shown, || store
                .artifacts(&session, "session"))
            .is_err());
        assert!(!store
            .history(&session)
            .unwrap()
            .iter()
            .any(|(_, _, kind, _)| kind == "human.approved"));
        let event = store
            .history(&session)
            .unwrap()
            .into_iter()
            .find(|(id, _, _, _)| *id == manifest)
            .unwrap();
        let pinned: InputManifest = serde_json::from_str(&event.3).unwrap();
        assert_eq!(pinned.artifacts[0].event_id, shown[0].event_id);
    }

    #[test]
    fn forked_approval_visibility_respects_cutoffs_and_reopening() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("demo", "goal", "approve").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "original")
            .unwrap();
        let before = store.history(&session).unwrap().last().unwrap().0;
        let approved = approve(&store, &session);
        let after = store.history(&session).unwrap().last().unwrap().0;
        let parent = session.active_branch.clone();
        store.fork(&mut session, before, "implement").unwrap();
        assert!(store
            .require_approved_input(&session, "approve", &approved)
            .is_err());
        store
            .switch_branch(&mut session, &parent, "implement")
            .unwrap();
        store.fork(&mut session, after, "implement").unwrap();
        drop(store);
        let reopened = Store::open(root.path()).unwrap();
        reopened
            .require_approved_input(&session, "approve", &approved)
            .unwrap();
        let history = reopened.history(&session).unwrap();
        let approval = history
            .iter()
            .find(|(_, _, kind, _)| kind == "human.approved")
            .unwrap();
        let pinned: Approval = serde_json::from_str(&approval.3).unwrap();
        assert_eq!(pinned.inputs.artifacts[0].event_id, approved.event_id);
        assert!(history
            .iter()
            .any(|(id, _, kind, _)| *id == pinned.input_manifest && kind == "context.inputs"));
    }

    #[test]
    fn unfinished_or_legacy_approvals_do_not_authorize_downstream_inputs() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "approve").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "original")
            .unwrap();
        let input = store.artifacts(&session, "session").unwrap().remove(0);
        store
            .begin_attempt(&session, "approve", "definition", "fixture")
            .unwrap();
        store
            .event(&session, Some("approve"), "human.approved", "")
            .unwrap();
        assert!(store
            .require_approved_input(&session, "approve", &input)
            .is_err());
        store
            .event(&session, Some("approve"), "step.completed", "")
            .unwrap();
        assert!(store
            .require_approved_input(&session, "approve", &input)
            .is_err());
    }

    #[test]
    fn tui_gate_rejects_stale_prompt_then_implementation_uses_reapproved_version() {
        use std::{fs, time::Duration};
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: fixture\n  base_url: http://127.0.0.1:9\n",
        )
        .unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: approve\nsteps: [approve, implement]\n",
        )
        .unwrap();
        fs::write(folder.join("approve.md"),"---\nid: approve\nkind: human\ninputs:\n  required: [plan.md]\nverification:\n  type: none\ntransitions:\n  success: implement\n---\nReview plan\n").unwrap();
        fs::write(folder.join("implement.md"),"---\nid: implement\nmax_turns: 0\ninputs:\n  required: [plan.md]\nverification:\n  type: none\n---\nImplement the plan\n").unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "approve").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "ORIGINAL_PLAN")
            .unwrap();
        let runner =
            crate::tui_actions::resume(root.path().into(), session.id.clone(), String::new());
        let prompt = runner.prompts.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(prompt.content.contains("ORIGINAL_PLAN"));
        store
            .add_artifact(&session, "plan", "plan.md", "EDITED_PLAN")
            .unwrap();
        runner.answers.send("y".into()).unwrap();
        runner
            .finished
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(store.session(&session.id).unwrap().current_step, "approve");
        let fresh =
            crate::tui_actions::resume(root.path().into(), session.id.clone(), String::new());
        let prompt = fresh.prompts.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(prompt.content.contains("EDITED_PLAN"));
        fresh.answers.send("y".into()).unwrap();
        fresh
            .finished
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let state = store.session(&session.id).unwrap();
        assert_eq!(state.current_step, "implement");
        let history = store.history(&state).unwrap();
        let context = history
            .iter()
            .rev()
            .find(|(_, _, kind, _)| kind == "context.assembled")
            .unwrap();
        assert!(context.3.contains("EDITED_PLAN"));
        assert!(!context.3.contains("ORIGINAL_PLAN"));
        let manifest = history
            .iter()
            .rev()
            .find(|(_, step, kind, _)| step == "implement" && kind == "context.inputs")
            .unwrap();
        let pinned: InputManifest = serde_json::from_str(&manifest.3).unwrap();
        assert_eq!(pinned.approval_events.len(), 1);
        assert!(history
            .iter()
            .any(|(id, _, kind, _)| *id == pinned.approval_events[0] && kind == "human.approved"));
        store
            .add_artifact(&state, "plan", "plan.md", "UNAPPROVED_PLAN")
            .unwrap();
        let blocked =
            crate::tui_actions::resume(root.path().into(), state.id.clone(), String::new());
        blocked
            .finished
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let history = store.history(&store.session(&state.id).unwrap()).unwrap();
        assert!(history
            .iter()
            .rev()
            .find(|(_, _, kind, _)| kind == "step.error")
            .unwrap()
            .3
            .contains("reapprove"));
        assert!(!history
            .iter()
            .filter(|(_, _, kind, _)| kind == "context.assembled")
            .any(|(_, _, _, text)| text.contains("UNAPPROVED_PLAN")));
        // Reapprove, then have the agent replace its own approved input. Its
        // next proposed native write must be rejected before touching disk.
        let mut state = store.session(&state.id).unwrap();
        let branch = state.active_branch.clone();
        store.switch_branch(&mut state, &branch, "approve").unwrap();
        let gate = crate::tui_actions::resume(root.path().into(), state.id.clone(), String::new());
        gate.prompts.recv_timeout(Duration::from_secs(5)).unwrap();
        gate.answers.send("y".into()).unwrap();
        gate.finished
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let implementation = folder.join("implement.md");
        fs::write(
            &implementation,
            fs::read_to_string(&implementation)
                .unwrap()
                .replace("max_turns: 0", "max_turns: 3"),
        )
        .unwrap();
        let mut engine = crate::engine::Engine::open(root.path()).unwrap();
        engine.tools.policies.write = "allow".into();
        engine.provider.script = Some(std::sync::Arc::new(std::sync::Mutex::new(
            crate::provider::Script {
                responses: vec![
                    Ok("action: artifact\nname: plan.md\ncontent: AGENT_REPLACEMENT".into()),
                    Ok(
                        "action: tool\ntool: write\npath: must-not-exist.txt\ncontent: forbidden"
                            .into(),
                    ),
                ]
                .into(),
                requests: Vec::new(),
            },
        )));
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(engine.run(&state.id, None, None))
            .unwrap();
        assert!(!root.path().join("must-not-exist.txt").exists());
        assert_eq!(store.session(&state.id).unwrap().status, "blocked");
    }
}
