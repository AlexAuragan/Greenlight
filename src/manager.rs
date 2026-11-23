//! Bounded, read-only coordinator Q&A. A validated forward is a proposal only.
use crate::{
    config,
    engine::Engine,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};
use uuid::Uuid;

pub const MESSAGE_BYTES: usize = 12 * 1024;
const SNAPSHOT_BYTES: usize = 40 * 1024;
const INPUT_BYTES: usize = 64 * 1024;
const PROMPT_DOCUMENT: &str = include_str!("../docs/prompts/manager-v1.md");
pub fn system_prompt() -> &'static str {
    PROMPT_DOCUMENT
        .split_once("```text\n")
        .expect("manager prompt block")
        .1
        .split_once("```")
        .expect("manager prompt closing fence")
        .0
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn prefix(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
pub fn read_message_file(path: &Path) -> Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MESSAGE_BYTES as u64 + 1)
        .read_to_string(&mut text)?;
    validate_message(&text)?;
    Ok(text)
}
fn validate_message(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > MESSAGE_BYTES {
        bail!("Manager message must be nonempty and at most 12 KiB; it was not truncated");
    }
    Ok(())
}

pub struct SavedMessage {
    pub message_id: String,
    pub session: Session,
    pub text: String,
}
pub fn save_message(store: &Store, session: &str, text: &str) -> Result<SavedMessage> {
    save_clarification(store, session, text, None)
}

/// A reply is a new immutable message linked to one visible clarification.
pub fn save_clarification(
    store: &Store,
    session: &str,
    text: &str,
    parent: Option<i64>,
) -> Result<SavedMessage> {
    validate_message(text)?;
    let tx = store.db.unchecked_transaction()?;
    let s = store.session(session)?;
    let mut thread = Vec::<Value>::new();
    if let Some(event) = parent {
        let history = store.history(&s)?;
        let raw = &history
            .iter()
            .find(|e| e.0 == event && e.2 == "manager.result")
            .context("Clarification must be a visible manager result on this branch")?
            .3;
        let reply: Reply = serde_json::from_str(raw)?;
        let Decision::Clarify {
            content,
            message_id,
            ..
        } = &reply.decision
        else {
            bail!("Selected manager result is not a clarification");
        };
        let original = history
            .iter()
            .filter(|e| e.0 < event && e.2 == "manager.message")
            .find_map(|e| {
                serde_json::from_str::<Value>(&e.3)
                    .ok()
                    .filter(|v| v["message_id"].as_str() == Some(message_id))
            })
            .context("Clarification original unavailable")?;
        thread = serde_json::from_value(
            original
                .get("clarification_thread")
                .cloned()
                .unwrap_or(json!([])),
        )?;
        thread.push(json!({"message_id":message_id,"text":original["text"],"question_event":event,"question":content}));
        if thread.len() > 8 || serde_json::to_vec(&thread)?.len() > 24 * 1024 {
            bail!("Clarification thread exceeds 8 turns or 24 KiB; start a new explicit request");
        }
    }
    let message_id = Uuid::new_v4().to_string();
    store.event(
        &s,
        None,
        "manager.message",
        &json!({"message_id":message_id,"text":text,"clarification_thread":thread}).to_string(),
    )?;
    tx.commit()?;
    Ok(SavedMessage {
        message_id,
        session: s,
        text: text.into(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub source_id: String,
    pub origin: String,
    pub captured_at: String,
    pub sha256_captured_text: String,
    pub truncated: bool,
    pub body: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub snapshot_id: String,
    pub message_id: String,
    pub session: Session,
    pub captured_at: String,
    pub event_cutoff: i64,
    pub control_fingerprint: String,
    pub prompt_sha256: String,
    pub provider: String,
    pub model: String,
    pub owner_observation: String,
    pub live_delivery_supported: bool,
    #[serde(default)]
    pub route_execution_supported: bool,
    pub valid_steps: BTreeMap<String, String>,
    pub sources: Vec<Source>,
    pub omissions: Vec<String>,
}
impl Snapshot {
    fn add(&mut self, id: String, origin: String, body: &str, limit: usize) -> Result<()> {
        if self.sources.iter().any(|s| s.source_id == id) {
            return Ok(());
        }
        let captured = prefix(body, limit).to_owned();
        let entry = Source {
            source_id: id.clone(),
            origin,
            captured_at: self.captured_at.clone(),
            sha256_captured_text: hash(captured.as_bytes()),
            truncated: captured.len() != body.len(),
            body: captured,
        };
        self.sources.push(entry);
        if self.sources.len() > 20 || serde_json::to_vec(self)?.len() > SNAPSHOT_BYTES - 2048 {
            self.sources.pop();
            self.omissions
                .push(format!("{id}: omitted by source/input bound"));
        }
        Ok(())
    }
}

pub(crate) fn workflow_digest(wf: &config::LoadedWorkflow) -> Result<String> {
    let steps: BTreeMap<_, _> = wf.steps.iter().collect();
    Ok(hash(&serde_json::to_vec(
        &json!({"workflow":wf.definition,"steps":steps}),
    )?))
}
pub(crate) fn control(store: &Store, s: &Session, wf: &config::LoadedWorkflow) -> Result<String> {
    let definition = workflow_digest(wf)?;
    let token: Option<String> = store
        .db
        .query_row(
            "SELECT token FROM execution_owners WHERE session=?1",
            [&s.id],
            |r| r.get(0),
        )
        .optional()?;
    let attempts = store.attempts(s)?;
    let last = attempts
        .last()
        .map(|a| json!({"id":a.id,"start":a.start_event,"end":a.end_event,"status":a.status}));
    let evidence_cutoff = store
        .history(s)?
        .iter()
        .filter(|e| {
            matches!(
                e.2.as_str(),
                "artifact.created"
                    | "human.approved"
                    | "human.declined"
                    | "verification.human"
                    | "verification.command"
                    | "verification.aggregate"
                    | "file.changed"
                    | "workspace.restored"
                    | "context.inputs"
                    | "user.artifact_edited"
            )
        })
        .map(|e| e.0)
        .max();
    Ok(hash(&serde_json::to_vec(
        &json!({"session":s,"definition":definition,"owner":token,"attempt":last,"native_recovery_clear":store.ensure_no_pending_writes().is_ok(),"evidence_cutoff":evidence_cutoff}),
    )?))
}

pub fn capture(
    root: &Path,
    store: &Store,
    session: &str,
    message_id: &str,
    artifacts: &[String],
) -> Result<Snapshot> {
    let cfg = config::load_config(root)?;
    capture_with_config(root, store, session, message_id, artifacts, &cfg)
}
pub(crate) fn capture_with_config(
    root: &Path,
    store: &Store,
    session: &str,
    message_id: &str,
    artifacts: &[String],
    cfg: &config::Config,
) -> Result<Snapshot> {
    if artifacts.len() > 4 {
        bail!("Select at most four artifacts for manager context");
    }
    let tx = store.db.unchecked_transaction()?;
    let s = store.session(session)?;
    let history = store.history(&s)?;
    let wf = config::load_workflow(root, &s.workflow)?;
    let now = chrono::Utc::now().to_rfc3339();
    let model = cfg
        .manager
        .model
        .as_deref()
        .or(cfg.model_roles.manager.as_deref())
        .unwrap_or(&cfg.provider.model)
        .to_owned();
    let provider = reqwest::Url::parse(&cfg.provider.base_url)
        .map(|v| v.origin().ascii_serialization())
        .unwrap_or_else(|_| "unknown provider".into());
    let mut snapshot = Snapshot {
        snapshot_id: Uuid::new_v4().to_string(),
        message_id: message_id.into(),
        session: s.clone(),
        captured_at: now,
        event_cutoff: history.last().map(|e| e.0).unwrap_or(0),
        control_fingerprint: control(store, &s, &wf)?,
        prompt_sha256: hash(system_prompt().as_bytes()),
        provider,
        model,
        owner_observation: format!("{:?}", store.execution_state(&s.id)?),
        live_delivery_supported: store.steering(&s)?.is_some()
            && store.execution_state(&s.id)? == crate::session_execution::ExecutionState::Live,
        route_execution_supported: true,
        valid_steps: wf
            .steps
            .iter()
            .map(|(id, step)| (id.clone(), step.kind.clone()))
            .collect(),
        sources: Vec::new(),
        omissions: Vec::new(),
    };
    let steps: BTreeMap<_, _> = wf.steps.iter().collect();
    snapshot.add(
        format!("workflow:{}", s.workflow),
        "Selected on-disk workflow, not a saved active-attempt definition".into(),
        &serde_json::to_string(&json!({"definition":wf.definition,"steps":steps}))?,
        8192,
    )?;
    let attempts = store.attempts(&s)?;
    let progress:Vec<_>=attempts.iter().rev().take(20).map(|a|json!({"attempt":a.id,"step":a.step,"start_event":a.start_event,"end_event":a.end_event,"status":a.status,"definition_event":a.definition_event})).collect();
    snapshot.add(
        "pipeline:attempts".into(),
        "Recorded branch-visible attempts; absent attempts are not inferred".into(),
        &serde_json::to_string(&progress)?,
        4096,
    )?;
    if let Some(a) = attempts.last() {
        if let Some(e) = history.iter().find(|e| e.0 == a.definition_event) {
            snapshot.add(
                format!("event:{}", e.0),
                format!("Saved definition of attempt {}", a.id),
                &e.3,
                6144,
            )?;
        }
        if let Some(e) = history.iter().rev().find(|e| {
            e.0 > a.start_event
                && a.end_event.is_none_or(|end| e.0 <= end)
                && e.2 == "context.inputs"
        }) {
            snapshot.add(
                format!("event:{}", e.0),
                format!(
                    "Exact pinned input/approval references for attempt {}",
                    a.id
                ),
                &e.3,
                4096,
            )?;
        }
    }
    let deliveries=store.manager_routes(&s)?.into_iter().rev().take(10).map(|r|json!({"message_id":r.message_id,"step":r.step,"state":r.state,"attempt":r.attempt,"first_prepared_call":r.call})).collect::<Vec<_>>();
    snapshot.add(
        "pipeline:delivery".into(),
        "Recorded idle-routing state; inclusion is not provider acceptance or work completion"
            .into(),
        &serde_json::to_string(&deliveries)?,
        4096,
    )?;
    snapshot.omissions.push("Unanswered approval prompts are UI-local and not persisted; their pending state is unknown to this snapshot.".into());
    snapshot.add("pipeline:safety".into(),"Read-only control observations; no permission decision is supplied".into(),&json!({"owner_observation":snapshot.owner_observation,"native_recovery_check_clear":store.ensure_no_pending_writes().is_ok(),"if_not_clear":"Pending writes or an unavailable recovery check; inspect deterministic recovery controls before execution","pending_approval":"Not persisted; unknown","route_execution_supported":snapshot.route_execution_supported,"live_delivery_supported":snapshot.live_delivery_supported}).to_string(),2048)?;
    // A fixed source allowlist; no model-selected paths or provider/approval files.
    for relative in ["README.md", "TODO.md", ".greenlight/instructions.md"] {
        let data = (|| -> Result<(String, bool)> {
            crate::restore::checked_path(root, relative)?;
            let file = crate::anchored_fs::Dir::open(root)?.open_file(Path::new(relative))?;
            let mut bytes = Vec::new();
            let read_limit = if relative == "README.md" {
                128 * 1024
            } else {
                6144
            };
            file.take(read_limit + 4).read_to_end(&mut bytes)?;
            let valid = match std::str::from_utf8(&bytes) {
                Ok(v) => v,
                Err(e) if e.error_len().is_none() && bytes.len() as u64 == read_limit + 4 => {
                    std::str::from_utf8(&bytes[..e.valid_up_to()])?
                }
                Err(_) => bail!("Source is not UTF-8"),
            };
            Ok((valid.to_owned(), bytes.len() as u64 > read_limit))
        })();
        match data {
            Ok((body, truncated)) => {
                if relative == "README.md" {
                    let mut offset = 0;
                    for line in body.split_inclusive('\n') {
                        let lower = line.trim_start().to_ascii_lowercase();
                        if (lower.starts_with('#')
                            && (lower.contains("todo") || lower.contains("roadmap")))
                            || lower.starts_with("todo:")
                        {
                            if offset >= 6144 {
                                let id = "file:README.md#todo".to_owned();
                                snapshot.add(id.clone(),format!("README roadmap/TODO excerpt starting at byte {offset}; search bounded to 128 KiB; independent file observation"),&body[offset..],12*1024)?;
                                if let Some(source) =
                                    snapshot.sources.iter_mut().find(|s| s.source_id == id)
                                {
                                    source.truncated |= truncated;
                                }
                            }
                            break;
                        }
                        offset += line.len();
                    }
                }
                let id = format!("file:{relative}");
                snapshot.add(
                    id.clone(),
                    format!("Captured project document {relative}; not atomic with database state"),
                    &body,
                    6144,
                )?;
                if let Some(source) = snapshot.sources.iter_mut().find(|v| v.source_id == id) {
                    source.truncated |= truncated;
                }
            }
            Err(_) => snapshot.omissions.push(format!(
                "{relative}: missing, unsafe or unreadable; not included"
            )),
        }
    }
    for name in artifacts {
        let selected = (|| -> Result<_> {
            let v = store
                .artifact_versions(&s, name)?
                .into_iter()
                .next()
                .context("No visible version")?;
            store.artifact_version(&s, name, v.event_id)
        })();
        match selected {
            Ok(a) => snapshot.add(
                format!("artifact:{}", a.event_id),
                format!(
                    "Explicitly selected {} at creation #{} from {}",
                    a.name, a.event_id, a.step
                ),
                &a.content,
                6144,
            )?,
            Err(_) => snapshot
                .omissions
                .push("A selected artifact version/body was unavailable".into()),
        }
    }
    let projection = crate::activity_cards::Projection::build(&history);
    let recent = history
        .iter()
        .rev()
        .filter(|e| {
            matches!(
                e.2.as_str(),
                "step.completed"
                    | "step.blocked"
                    | "step.error"
                    | "step.deviation"
                    | "verification.passed"
                    | "verification.failed"
                    | "verification.command"
                    | "verification.agent"
                    | "artifact.created"
                    | "tool.error"
                    | "workspace.correlation"
                    | "human.approved"
                    | "runner.recovered"
            )
        })
        .take(8)
        .collect::<Vec<_>>();
    for e in recent {
        let card = projection.cards.get(&e.0);
        let body = if matches!(e.2.as_str(), "verification.command" | "tool.error") {
            format!(
                "{}\n{}",
                card.map(|c| format!("{}: {}", c.title, c.summary))
                    .unwrap_or_default(),
                prefix(&e.3, 2048)
            )
        } else {
            prefix(&e.3, 2048).to_owned()
        };
        snapshot.add(
            format!("event:{}", e.0),
            format!(
                "Recorded {} in step {}; model prose is not a verification verdict",
                e.2, e.1
            ),
            &body,
            3072,
        )?;
    }
    let mut history_bytes = 0;
    for e in history
        .iter()
        .rev()
        .filter(|e| e.2 == "manager.result")
        .take(4)
    {
        if let Ok(reply) = serde_json::from_str::<Reply>(&e.3) {
            let original = history.iter().find(|v| {
                v.2 == "manager.message"
                    && serde_json::from_str::<serde_json::Value>(&v.3)
                        .ok()
                        .is_some_and(|v| v["message_id"] == reply.decision.message_id())
            });
            let question = original
                .and_then(|v| serde_json::from_str::<serde_json::Value>(&v.3).ok())
                .and_then(|v| v["text"].as_str().map(str::to_owned))
                .unwrap_or_default();
            let body = serde_json::to_string(
                &json!({"question":prefix(&question,1024),"prior_manager_interpretation":prefix(reply.decision.content(),2048),"status":reply.status}),
            )?;
            history_bytes += body.len();
            if history_bytes > 16 * 1024 {
                break;
            }
            snapshot.add(
                format!("manager-history:{}", e.0),
                "Prior manager exchange, not execution evidence".into(),
                &body,
                4096,
            )?;
        }
    }
    if serde_json::to_vec(&snapshot)?.len() > SNAPSHOT_BYTES {
        bail!("Manager snapshot exceeds input bound; no inference sent");
    }
    tx.commit()?;
    Ok(snapshot)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Answer {
        schema_version: u8,
        message_id: String,
        snapshot_id: String,
        content: String,
        citations: Vec<String>,
    },
    Forward {
        schema_version: u8,
        message_id: String,
        snapshot_id: String,
        content: String,
        citations: Vec<String>,
        step_id: String,
    },
    Clarify {
        schema_version: u8,
        message_id: String,
        snapshot_id: String,
        content: String,
        citations: Vec<String>,
    },
}
impl Decision {
    fn fields(&self) -> (u8, &str, &str, &str, &[String]) {
        match self {
            Self::Answer {
                schema_version,
                message_id,
                snapshot_id,
                content,
                citations,
            }
            | Self::Forward {
                schema_version,
                message_id,
                snapshot_id,
                content,
                citations,
                ..
            }
            | Self::Clarify {
                schema_version,
                message_id,
                snapshot_id,
                content,
                citations,
            } => (*schema_version, message_id, snapshot_id, content, citations),
        }
    }
    pub fn snapshot_id(&self) -> &str {
        self.fields().2
    }
    pub fn message_id(&self) -> &str {
        self.fields().1
    }
    pub fn content(&self) -> &str {
        self.fields().3
    }
    pub fn parse(raw: &str, snapshot: &Snapshot) -> Result<Self> {
        if raw.len() > 32 * 1024 {
            bail!("Manager response exceeds limit");
        }
        let d: Self = serde_json::from_str(raw).context("Invalid manager JSON decision")?;
        let (version, message, snap, content, citations) = d.fields();
        if version != 1 || message != snapshot.message_id || snap != snapshot.snapshot_id {
            bail!("Manager response has incompatible version or IDs");
        }
        let max = if matches!(d, Self::Answer { .. }) {
            4000
        } else {
            500
        };
        if content.trim().is_empty()
            || content.chars().count() > max
            || content
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            bail!("Manager display content is invalid");
        }
        let unique: BTreeSet<_> = citations.iter().collect();
        if citations.len() > 10
            || unique.len() != citations.len()
            || citations
                .iter()
                .any(|id| !snapshot.sources.iter().any(|s| s.source_id == *id))
        {
            bail!("Invalid manager citation references");
        }
        if let Self::Forward { step_id, .. } = &d {
            if snapshot.valid_steps.get(step_id).map(String::as_str) != Some("agent") {
                bail!("Manager destination is not a supplied agent step");
            }
        }
        // Answers without sources cannot pass as grounded facts; use clarify for gaps.
        if matches!(d, Self::Answer { .. }) && citations.is_empty() {
            bail!("Manager answers require supplied evidence citations");
        }
        Ok(d)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub decision: Decision,
    pub status: String,
    pub snapshot_changed: bool,
    pub captured_at: String,
    pub sources: Vec<Source>,
}
impl Reply {
    pub fn display(&self) -> String {
        let mut text = format!(
            "Manager · {} · as of {}\n\n{}\n",
            self.status,
            self.captured_at,
            self.decision.content()
        );
        if let Decision::Forward { step_id, .. } = &self.decision {
            text.push_str(&format!("\nProposed destination: {step_id}. Use the explicit Deliver original action for idle routing, or inspect the guarded restart proposal for an already visited target. Original message {} is retained; this answer alone did not resume, queue or restart work.\n",self.decision.message_id()));
        }
        if self.snapshot_changed {
            text.push_str("\nPipeline/workflow state changed during inference. This answer is a snapshot interpretation; any proposed route needs review.\n");
        }
        text.push_str("\nCited captured sources (may be truncated; no further files were read):\n");
        for source in &self.sources {
            text.push_str(&format!(
                "\n{} · {} · captured-text SHA-256 {}{}\n{}\n",
                source.source_id,
                source.origin,
                source.sha256_captured_text,
                if source.truncated {
                    " · truncated"
                } else {
                    ""
                },
                source.body
            ));
        }
        crate::markdown::raw(&text)
            .lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Engine {
    pub async fn ask_manager(
        &self,
        session: &str,
        text: &str,
        artifacts: &[String],
    ) -> Result<Reply> {
        let saved = save_message(&self.store, session, text)?;
        self.ask_saved_manager(&saved, artifacts).await
    }
    pub async fn ask_saved_manager(
        &self,
        saved: &SavedMessage,
        artifacts: &[String],
    ) -> Result<Reply> {
        let _lock = self.store.manager_lock(&saved.session.id)?;
        let s = &saved.session;
        let result = self
            .manager_exchange(s, &saved.message_id, &saved.text, artifacts)
            .await;
        if result.is_err() {
            self.store.event(s,None,"manager.failed",&json!({"message_id":saved.message_id,"status":"unresolved","message":"Manager could not return a validated decision. Original message retained; no execution was started. Inspect manager output/usage and provider diagnostics."}).to_string())?;
        }
        result
    }
    pub(crate) async fn manager_exchange(
        &self,
        s: &Session,
        message_id: &str,
        text: &str,
        artifacts: &[String],
    ) -> Result<Reply> {
        let restored = self.continuation.borrow().clone();
        let snapshot: Snapshot =
            if let Some(cp) = restored.as_ref().filter(|cp| cp.purpose == "manager") {
                let payload: serde_json::Value = serde_json::from_str(
                    cp.messages[1]["content"]
                        .as_str()
                        .context("Missing saved manager payload")?,
                )?;
                serde_json::from_value(payload["evidence"].clone())?
            } else {
                capture_with_config(
                    &self.root,
                    &self.store,
                    &s.id,
                    message_id,
                    artifacts,
                    &self.cfg,
                )?
            };
        if snapshot.session.active_branch != s.active_branch {
            bail!("Branch changed before manager snapshot; message retained for review");
        }
        self.store.event(
            s,
            None,
            "manager.snapshot",
            &serde_json::to_string(&snapshot)?,
        )?;
        let thread = self
            .store
            .history(s)?
            .iter()
            .filter(|e| e.2 == "manager.message")
            .find_map(|e| {
                serde_json::from_str::<Value>(&e.3)
                    .ok()
                    .filter(|v| v["message_id"].as_str() == Some(message_id))
            })
            .and_then(|v| v.get("clarification_thread").cloned())
            .unwrap_or(json!([]));
        let input = serde_json::to_string(
            &json!({"message_id":message_id,"text":text,"clarification_thread":thread,"evidence":snapshot}),
        )?;
        if input.len() + system_prompt().len() > INPUT_BYTES {
            bail!("Manager input exceeds 64 KiB; original message retained without inference");
        }
        let messages = [
            json!({"role":"system","content":system_prompt()}),
            json!({"role":"user","content":input}),
        ];
        let raw = self
            .model_call_with_message(
                s,
                &snapshot.session.current_step,
                "manager",
                &snapshot.model,
                &messages,
                Some((message_id, &snapshot.snapshot_id)),
            )
            .await?;
        self.store.event(s, None, "manager.output", &raw)?;
        let decision=Decision::parse(&raw,&snapshot).map_err(|_|anyhow::anyhow!("Manager returned invalid routing JSON. Original message retained; inspect manager.output in raw history. No action was executed."))?;
        let fresh = (|| -> Result<_> {
            let tx = self.store.db.unchecked_transaction()?;
            let current = self.store.session(&s.id)?;
            let wf = config::load_workflow(&self.root, &current.workflow)?;
            let fingerprint = control(&self.store, &current, &wf)?;
            tx.commit()?;
            Ok(fingerprint)
        })();
        let changed = fresh.ok().as_deref() != Some(snapshot.control_fingerprint.as_str());
        let status = match &decision {
            Decision::Answer { .. } => "answered",
            Decision::Clarify { .. } => "clarification_needed",
            Decision::Forward { .. } if changed => "needs_review",
            Decision::Forward { .. } => "forward_proposed",
        }
        .to_owned();
        let cited = &decision.fields().4;
        let sources = snapshot
            .sources
            .iter()
            .filter(|s| cited.contains(&s.source_id))
            .cloned()
            .collect();
        let reply = Reply {
            decision,
            status,
            snapshot_changed: changed,
            captured_at: snapshot.captured_at,
            sources,
        };
        self.store
            .event(s, None, "manager.result", &serde_json::to_string(&reply)?)?;
        Ok(reply)
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
