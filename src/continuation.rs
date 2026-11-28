//! Durable provider-call checkpoints. Never persist credentials or unanswered approvals.
use crate::{
    engine::{Engine, Outcome},
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Default, Serialize, Deserialize)]
pub enum Frame {
    #[default]
    None,
    Coding {
        messages: Vec<Value>,
        response: Option<(String, i64)>,
    },
    Verify {
        summary: String,
    },
    Deviation {
        outcome: Outcome,
    },
    Manager,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub id: String,
    pub session: String,
    pub branch: String,
    pub step: String,
    pub attempt: Option<String>,
    pub purpose: String,
    pub model: String,
    pub messages: Vec<Value>,
    pub interaction: Option<(String, String)>,
    pub frame: Frame,
    pub checks: Vec<Result<(), String>>,
    pub fingerprint: String,
    pub cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    manager: bool,
    blob: Option<String>,
}
pub fn pending_reference(
    history: &[(i64, String, String, String)],
    manager: bool,
) -> Result<Option<String>> {
    for event in history
        .iter()
        .rev()
        .filter(|event| event.2 == "continuation.state")
    {
        let value: Value = serde_json::from_str(&event.3)?;
        if value.get("blob").is_none() {
            bail!("Saved continuation reference is missing its blob field");
        }
        let reference: Reference = serde_json::from_value(value)?;
        if reference.manager == manager {
            return Ok(reference.blob);
        }
    }
    Ok(None)
}
impl Store {
    pub(crate) fn manager_lock(&self, id: &str) -> Result<crate::locks::LockGuard> {
        let name = format!(
            "manager-continuation-{:x}.lock",
            Sha256::digest(id.as_bytes())
        );
        let file = self.directory_fd.lock_file(std::path::Path::new(&name))?;
        crate::locks::LockGuard::acquire(file)
            .context("A manager request is already running for this session")
    }
    pub fn pending_continuation(&self, s: &Session, manager: bool) -> Result<Option<Checkpoint>> {
        pending_reference(&self.history(s)?, manager)?
            .map(|id| {
                serde_json::from_value(crate::conversation_dump::load_message(self, &id)?)
                    .context("Invalid saved continuation")
            })
            .transpose()
    }
    pub fn save_continuation(&self, s: &Session, cp: &Checkpoint) -> Result<()> {
        let blob = crate::conversation_dump::save_message(self, &serde_json::to_value(cp)?)?;
        self.event(
            s,
            Some(&cp.step),
            "continuation.state",
            &serde_json::to_string(&Reference {
                manager: cp.purpose == "manager",
                blob: Some(blob),
            })?,
        )?;
        Ok(())
    }
    pub fn close_continuation(&self, s: &Session, manager: bool) -> Result<()> {
        self.event(
            s,
            None,
            "continuation.state",
            &serde_json::to_string(&Reference {
                manager,
                blob: None,
            })?,
        )?;
        Ok(())
    }
}
impl Engine {
    fn continuation_fingerprint(&self, s: &Session) -> Result<String> {
        let scan = crate::workspace::scan(&self.root)?;
        let controls: Vec<_> = self
            .store
            .history(s)?
            .into_iter()
            .filter(|e| {
                matches!(
                    e.2.as_str(),
                    "verification.human"
                        | "verification.command"
                        | "verification.workspace"
                        | "verification.diagnostic"
                        | "verification.imported"
                        | "dependency.prepared"
                        | "dependency.cleared"
                        | "artifact.created"
                        | "human.approved"
                        | "human.declined"
                        | "file.changed"
                        | "workspace.restored"
                        | "tool.rerun"
                        | "step.completed"
                        | "branch.forked"
                        | "manager.restart-authorized"
                        | "manager.progress-resumed"
                )
            })
            .collect();
        let workflow = crate::config::load_workflow(&self.root, &s.workflow)?;
        let mut definitions: Vec<_> = workflow.steps.values().collect();
        definitions.sort_by(|a, b| a.id.cmp(&b.id));
        // Budget changes authorize dispatch capacity, not different prepared messages.
        let mut checkpoint_config = self.cfg.clone();
        checkpoint_config.budgets = Default::default();
        let bytes = serde_json::to_vec(&(
            s.workflow.as_str(),
            s.current_step.as_str(),
            s.goal.as_str(),
            scan.files,
            controls,
            definitions,
            &self.cfg.provider.base_url,
            self.cfg.provider.max_tokens,
            &checkpoint_config,
            crate::agent_protocol::SYSTEM,
            crate::manager::system_prompt(),
        ))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    /// Save only at a confirmed coding boundary, never an unfinished tool.
    pub(crate) fn checkpoint_interrupted_coding(
        &self,
        s: &Session,
        step: &str,
        model: &str,
        messages: &[Value],
        response: Option<(String, i64)>,
    ) -> Result<()> {
        self.store.ensure_no_pending_writes()?;
        crate::sandbox_job::ensure_clean(&self.store.directory_fd)?;
        *self.frame.borrow_mut() = Frame::Coding {
            messages: messages.to_vec(),
            response,
        };
        self.checkpoint(s, step, "coding", model, messages, None)?;
        Ok(())
    }
    pub(crate) fn checkpoint(
        &self,
        s: &Session,
        step: &str,
        purpose: &str,
        model: &str,
        messages: &[Value],
        interaction: Option<(&str, &str)>,
    ) -> Result<Option<Checkpoint>> {
        let frame = if purpose == "manager" && interaction.is_some() {
            Frame::Manager
        } else {
            self.frame.borrow().clone()
        };
        if matches!(frame, Frame::None) {
            return Ok(None);
        }
        let cp = Checkpoint {
            // Older binaries must refuse this new consume-before-action
            // continuation instead of replaying it with their old semantics.
            schema_version: if purpose == "coding"
                && matches!(
                    &frame,
                    Frame::Coding {
                        response: Some(_),
                        ..
                    }
                ) {
                2
            } else {
                1
            },
            id: uuid::Uuid::new_v4().to_string(),
            session: s.id.clone(),
            branch: s.active_branch.clone(),
            step: step.into(),
            attempt: self.store.active_attempt(s, step).ok().map(|a| a.0),
            purpose: purpose.into(),
            model: model.into(),
            messages: messages.to_vec(),
            interaction: interaction.map(|(a, b)| (a.into(), b.into())),
            frame,
            checks: self.checks.borrow().clone(),
            fingerprint: self.continuation_fingerprint(s)?,
            cooldown_until: None,
        };
        self.store.save_continuation(s, &cp)?;
        Ok(Some(cp))
    }
    pub(crate) fn validate_continuation(&self, s: &Session, cp: &Checkpoint) -> Result<()> {
        if !matches!(cp.schema_version, 1 | 2) {
            bail!("Unsupported saved continuation version; checkpoint retained");
        }
        let current = self.store.session(&s.id)?;
        if current.active_branch != s.active_branch
            || current.current_step != s.current_step
            || current.workflow != s.workflow
            || current.goal != s.goal
        {
            bail!("Session changed; refresh before retrying the saved request");
        }
        self.store.ensure_no_pending_writes()?;
        if self
            .store
            .pending_continuation(s, cp.purpose == "manager")?
            .as_ref()
            .map(|saved| saved.id.as_str())
            != Some(cp.id.as_str())
        {
            bail!("Saved request was replaced or completed; refresh before retrying");
        }
        if !crate::workspace::scan(&self.root)?.skipped.is_empty() {
            bail!("Workspace scan omitted files; saved request cannot safely continue. Start a fresh attempt.");
        }
        if cp.session != s.id
            || cp.branch != s.active_branch
            || cp.step != s.current_step
            || cp.fingerprint != self.continuation_fingerprint(s)?
        {
            bail!("Workspace, workflow or recorded inputs changed since this request. Saved messages retained; start a fresh attempt instead.");
        }
        if cp.purpose != "manager"
            && cp.attempt.as_deref()
                != self
                    .store
                    .latest_attempt(s, &cp.step)
                    .ok()
                    .map(|a| a.0)
                    .as_deref()
        {
            bail!("The original attempt is no longer current; saved request cannot continue");
        }
        Ok(())
    }
    pub(crate) fn validate_pending_retry(&self, s: &Session, purpose: &str) -> Result<()> {
        // Summaries have frozen evidence, no executable continuation, and must not
        // consume or validate an unrelated paused workflow request.
        if purpose == "summarizer" {
            return Ok(());
        }
        if let Some(cp) = self.store.pending_continuation(s, purpose == "manager")? {
            if cp.purpose != purpose {
                bail!("Saved request phase changed before retry");
            }
            self.validate_continuation(s, &cp)?;
        }
        Ok(())
    }
    pub(crate) fn checkpoint_failure(
        &self,
        s: &Session,
        purpose: &str,
        failure: &crate::provider_failure::Failure,
    ) -> Result<()> {
        if let Some(mut cp) = self
            .store
            .pending_continuation(s, purpose == "manager")?
            .filter(|cp| cp.purpose == purpose)
        {
            cp.cooldown_until = failure.retry_after_seconds.map(|seconds| {
                chrono::Duration::try_seconds(seconds.min(i64::MAX as u64) as i64)
                    .and_then(|delay| chrono::Utc::now().checked_add_signed(delay))
                    .unwrap_or(chrono::DateTime::<chrono::Utc>::MAX_UTC)
            });
            self.store.save_continuation(s, &cp)?;
        }
        Ok(())
    }
    pub async fn resume_manager_saved(&self, id: &str) -> Result<crate::manager::Reply> {
        let s = self.store.session(id)?;
        let _lock = self.store.manager_lock(id)?;
        let cp = self
            .store
            .pending_continuation(&s, true)?
            .context("No saved manager request")?;
        self.validate_continuation(&s, &cp)?;
        let payload: Value = serde_json::from_str(
            cp.messages
                .get(1)
                .and_then(|message| message["content"].as_str())
                .context("Missing saved manager input")?,
        )?;
        let saved = crate::manager::SavedMessage {
            message_id: cp
                .interaction
                .as_ref()
                .context("Missing saved manager identity")?
                .0
                .clone(),
            session: s,
            text: payload["text"]
                .as_str()
                .context("Missing original manager message")?
                .into(),
        };
        *self.continuation.borrow_mut() = Some(cp);
        self.manager_exchange(&saved.session, &saved.message_id, &saved.text, &[])
            .await
    }
    pub async fn resume_saved(&mut self, id: &str) -> Result<()> {
        let s = self.store.session(id)?;
        let cp = self
            .store
            .pending_continuation(&s, false)?
            .context("No saved workflow request")?;
        // run_owned validates the checkpoint after claiming project/session ownership.

        *self.frame.borrow_mut() = cp.frame.clone();
        *self.checks.borrow_mut() = cp.checks.clone();
        *self.continuation.borrow_mut() = Some(cp);
        self.run(id, None, None).await
    }
}
