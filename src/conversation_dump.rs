//! Local forensic transcripts. Only returned model text is available, never private reasoning.
use crate::store::{Session, Store};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    io::Write,
    path::Path,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub branch: String,
    pub step: String,
    pub kind: String,
    pub timestamp: String,
    pub data: String,
}
#[derive(Clone, Serialize)]
pub struct Dump {
    #[serde(skip)]
    output_limit: usize,
    pub schema_version: u32,
    pub captured_at: String,
    pub session: Session,
    pub scope: String,
    pub events: Vec<Event>,
    /// Content-addressed exact message values referenced by model.request/model.response.
    pub messages: BTreeMap<String, Value>,
    pub diagnostics: Vec<String>,
    pub live_preview: Option<String>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn save_message(store: &Store, message: &Value) -> Result<String> {
    let bytes = crate::resource_limits::json_bytes(message, store.limits.blob_bytes)?;
    let id = hash(&bytes);
    let name = format!("trace-{id}.json");
    let directory = store.directory_fd.create_dirs(Path::new("blobs"))?;
    match directory.create_new(Path::new(&name)) {
        Ok(mut file) => {
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
        {
            if directory.read(Path::new(&name), store.limits.blob_bytes as u64)? != bytes {
                bail!("invalid existing trace blob");
            }
        }
        Err(error) => return Err(error),
    }
    directory.sync()?;
    Ok(id)
}
pub(crate) fn load_message(store: &Store, id: &str) -> Result<Value> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid message reference");
    }
    let bytes = store.directory_fd.open_dir(Path::new("blobs"))?.read(
        Path::new(&format!("trace-{id}.json")),
        store.limits.blob_bytes as u64,
    )?;
    if hash(&bytes) != id {
        bail!("trace blob checksum mismatch");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn record_request(
    store: &Store,
    s: &Session,
    step: &str,
    call: &str,
    purpose: &str,
    model: &str,
    messages: &[Value],
) -> Result<()> {
    let refs = messages
        .iter()
        .map(|m| save_message(store, m))
        .collect::<Result<Vec<_>>>()?;
    store.event(
        s,
        Some(step),
        "model.request",
        &json!({"schema_version":1,"call_id":call,"purpose":purpose,"model":model,"messages":refs})
            .to_string(),
    )?;
    Ok(())
}
pub fn record_response(
    store: &Store,
    s: &Session,
    call: &crate::usage::Call,
    elapsed_ms: u128,
    text: Option<&str>,
) -> Result<()> {
    let message = text
        .map(|text| save_message(store, &json!({"role":"assistant","content":text})))
        .transpose()?;
    store.event(s,Some(&call.step),"model.response",&json!({"schema_version":1,"call_id":call.id,"purpose":call.purpose,"status":call.status,"elapsed_ms":elapsed_ms,"message":message}).to_string())?;
    Ok(())
}

impl Dump {
    pub fn load(store: &Store, session: &str, branch: Option<&str>, all: bool) -> Result<Self> {
        let tx = store.db.unchecked_transaction()?;
        let s = store.session(session)?;
        let selected = branch.unwrap_or(&s.active_branch);
        let owner: String = store.db.query_row(
            "SELECT session FROM branches WHERE id=?1",
            [selected],
            |r| r.get(0),
        )?;
        if owner != s.id {
            bail!("branch belongs to another session");
        }
        let branches = if all {
            store
                .branches(&s.id)?
                .into_iter()
                .map(|b| (b.id, i64::MAX))
                .collect()
        } else {
            store.lineage(selected)?
        };
        let mut events = Vec::new();
        let mut budget = crate::resource_limits::Budget::new(store.limits.export_bytes);
        for (b, cutoff) in branches {
            let (count, bytes, largest): (usize, usize, usize) = store.db.query_row(
                "SELECT count(*),coalesce(sum(octet_length(data)+coalesce(octet_length(step),0)+octet_length(kind)+octet_length(branch)+128),0),coalesce(max(octet_length(data)),0) FROM events WHERE session=?1 AND branch=?2 AND id<=?3",
                rusqlite::params![s.id,b,cutoff], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            if count > store.limits.export_events.saturating_sub(events.len())
                || largest > store.limits.blob_bytes
            {
                bail!("Conversation exceeds limits.export_events/blob_bytes; recorded data is unchanged");
            }
            budget.charge(bytes)?;
            let mut query=store.db.prepare("SELECT id,branch,coalesce(step,''),kind,timestamp,data FROM events WHERE session=?1 AND branch=?2 AND id<=?3 ORDER BY id")?;
            events.extend(
                query
                    .query_map(rusqlite::params![s.id, b, cutoff], |r| {
                        Ok(Event {
                            id: r.get(0)?,
                            branch: r.get(1)?,
                            step: r.get(2)?,
                            kind: r.get(3)?,
                            timestamp: r.get(4)?,
                            data: r.get(5)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        events.sort_by_key(|e| e.id);
        let mut diagnostics=vec!["Only recorded evidence is available. Private reasoning is not returned or reconstructed. Live/in-flight text is not durable until a response is recorded.".into()];
        let mut messages = BTreeMap::new();
        let mut requests = HashSet::new();
        let mut responses = HashSet::new();
        for event in &events {
            if matches!(event.kind.as_str(), "model.request" | "model.response") {
                match serde_json::from_str::<Value>(&event.data) {
                    Ok(value) => {
                        if let Some(call) = value["call_id"].as_str() {
                            if event.kind == "model.request" {
                                requests.insert(call.to_owned());
                            } else {
                                responses.insert(call.to_owned());
                            }
                        }
                        let refs: Vec<&str> = if event.kind == "model.request" {
                            value["messages"]
                                .as_array()
                                .map(|a| a.iter().filter_map(Value::as_str).collect())
                                .unwrap_or_default()
                        } else {
                            value["message"].as_str().into_iter().collect()
                        };
                        for id in refs {
                            if !messages.contains_key(id) {
                                // Size-check before loading so an oversized trace is not misreported
                                // as a missing blob in an otherwise "full" export.
                                if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                                    if let Ok(file) =
                                        store.directory_fd.open_dir(Path::new("blobs")).and_then(
                                            |d| d.open_file(Path::new(&format!("trace-{id}.json"))),
                                        )
                                    {
                                        let bytes = usize::try_from(file.metadata()?.len())?;
                                        if bytes > store.limits.blob_bytes {
                                            bail!("Trace exceeds limits.blob_bytes; full dump refused, saved evidence is unchanged");
                                        }
                                        if bytes > budget.remaining() {
                                            bail!("Trace dictionary exceeds limits.export_bytes; full dump refused");
                                        }
                                    }
                                }
                                match load_message(store, id) {
                                    Ok(m) => {
                                        let bytes = crate::resource_limits::json_bytes(
                                            &m,
                                            budget.remaining(),
                                        )?;
                                        budget.charge(bytes.len())?;
                                        messages.insert(id.into(), m);
                                    }
                                    Err(e) => diagnostics.push(format!(
                                        "Event #{}: message {id} unavailable: {e}",
                                        event.id
                                    )),
                                }
                            }
                        }
                    }
                    Err(e) => diagnostics.push(format!(
                        "Event #{}: malformed trace metadata: {e}",
                        event.id
                    )),
                }
            }
            let label = match event.kind.as_str() {
                "protocol.error" => {
                    Some("Model action rejected by protocol parser; no tool execution established")
                }
                "tool.error" => {
                    Some("Tool action failed; inspect the recorded error and permission decision")
                }
                "provider.failure" => {
                    Some("Provider request failed; inspect failure category and retry events")
                }
                _ => None,
            };
            if let Some(label) = label {
                diagnostics.push(format!("#{} · {} · {label}", event.id, event.step));
            }
        }
        for id in requests
            .difference(&responses)
            .collect::<std::collections::BTreeSet<_>>()
        {
            diagnostics.push(format!(
                "Request {id}: no response trace recorded (in flight, interrupted or unavailable)."
            ));
        }
        let tuples = events
            .iter()
            .map(|e| (e.id, e.step.clone(), e.kind.clone(), e.data.clone()))
            .collect::<Vec<_>>();
        let projection = crate::activity_cards::Projection::build(&tuples);
        let mut correlated = HashSet::new();
        let mut evidence = projection.tools.values().collect::<Vec<_>>();
        evidence.sort_by_key(|e| e.metadata.request_event);
        for evidence in evidence {
            correlated.insert(evidence.metadata.request_event);
            if let Some(code) = evidence.metadata.exit_code.filter(|c| *c != 0) {
                diagnostics.push(format!("Tool request #{}: process exited with {code}; a tool.result does not mean the command succeeded.",evidence.metadata.request_event));
            }
        }
        for e in &events {
            if e.kind == "tool.request" && !correlated.contains(&e.id) {
                diagnostics.push(format!("Tool request #{}: no validated outcome link; legacy records may contain an unlinked result below. Execution is unknown.",e.id));
            }
        }
        if events
            .iter()
            .any(|e| e.kind == "model.call-started" && !requests.contains(&e.data))
            || events.iter().any(|e| e.kind == "model.output") && requests.is_empty()
        {
            diagnostics.push("Legacy run / incomplete trace: some exact prepared model messages are unavailable. Raw model.output, context, protocol corrections and tool events are retained below; missing payloads are not backfilled.".into());
        }
        tx.commit()?;
        Ok(Self {
            output_limit: store.limits.export_bytes,
            schema_version: 1,
            captured_at: chrono::Utc::now().to_rfc3339(),
            session: s.clone(),
            scope: if all {
                "all branches".into()
            } else {
                format!("branch {selected} and ancestry")
            },
            events,
            messages,
            diagnostics,
            live_preview: None,
        })
    }
    /// A bounded reader preview; the Markdown/JSON export retains complete bodies.
    pub fn preview(&self) -> String {
        self.render(true).unwrap_or_else(|e| {
            format!("Conversation preview unavailable: {e:#}. No evidence was deleted.")
        })
    }
    pub fn markdown(&self) -> Result<String> {
        self.render(false)
    }
    fn render(&self, preview: bool) -> Result<String> {
        let mut out = crate::resource_limits::Text::new(self.output_limit);
        out.push_str(&format!("# Conversation dump\n\nSession: {}\nWorkflow: {}\nGoal: {}\nScope: {}\nCaptured: {}\n\n",self.session.id,self.session.workflow,self.session.goal,self.scope,self.captured_at));
        out.push_str("## Diagnostics and limitations\n\n");
        for d in &self.diagnostics {
            out.push_str(&format!("- {d}\n"));
        }
        if let Some(live) = &self.live_preview {
            out.push_str("\n## Ephemeral live preview (not validated or executed evidence)\n\n");
            body(&mut out, live, preview);
        }
        out.push_str("\n## Recorded events (chronological)\n\n");
        for e in &self.events {
            out.push_str(&format!(
                "### #{} · {} · {}\n\n{} · branch {}\n\n",
                e.id, e.step, e.kind, e.timestamp, e.branch
            ));
            body(&mut out, &e.data, preview);
            if e.kind == "model.response" {
                if let Ok(value) = serde_json::from_str::<Value>(&e.data) {
                    if let Some(text) = value["message"]
                        .as_str()
                        .and_then(|id| self.messages.get(id))
                        .and_then(|m| m["content"].as_str())
                    {
                        out.push_str("Visible model response:\n\n");
                        body(&mut out, text, preview);
                    }
                }
            }
        }
        out.push_str("## Exact message dictionary\n\nmodel.request lists message hashes in exact prepared order. Reused messages are stored once. model.response references visible returned text. Provider-added chat template/hidden reasoning is unavailable.\n\n");
        for (id, message) in &self.messages {
            out.push_str(&format!("### Message {id}\n\n"));
            body(
                &mut out,
                &serde_json::to_string_pretty(message).unwrap_or_default(),
                preview,
            );
        }
        out.finish()
    }
    pub fn json(&self) -> Result<String> {
        Ok(String::from_utf8(crate::resource_limits::json_bytes(
            self,
            self.output_limit,
        )?)?)
    }
    pub fn save(&self, path: &Path, json: bool) -> Result<()> {
        let text = if json { self.json()? } else { self.markdown()? };
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let parent =
            crate::anchored_fs::Dir::open(absolute.parent().context("export lacks parent")?)?;
        parent
            .target(Path::new(
                absolute.file_name().context("export lacks filename")?,
            ))?
            .publish_new(text.as_bytes())
    }
}
fn body(out: &mut crate::resource_limits::Text, text: &str, preview: bool) {
    let excerpt = text.chars().take(800).collect::<String>();
    let long = excerpt.len() < text.len();
    if preview {
        out.push_str(&excerpt);
        if long {
            out.push_str("\n[… full body in saved dump]");
        }
        out.push_str("\n\n");
        return;
    }
    if long {
        let fence = "`".repeat(
            excerpt
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .max(2)
                + 1,
        );
        out.push_str(&format!("{fence}\n{excerpt}\n{fence}\n"));
        out.push_str("\n\n<details><summary>Full recorded body</summary>\n\n");
    }
    let fence = "`".repeat(
        text.split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1,
    );
    out.push_str(&format!("{fence}\n{text}\n{fence}\n\n"));
    if long {
        out.push_str("</details>\n\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Store, Session) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "diagnose tools", "work").unwrap();
        (root, store, s)
    }
    #[test]
    fn legacy_dump_retains_full_raw_evidence_and_flags_failures_without_claiming_execution() {
        let (_root, store, s) = fixture();
        let raw = format!("action: shell\n{}\nLAST_RAW_LINE", "é`<>".repeat(1000));
        store.event(&s, Some("work"), "model.output", &raw).unwrap();
        store
            .event(&s, Some("work"), "protocol.error", "unknown action shell")
            .unwrap();
        let request = store
            .event(
                &s,
                Some("work"),
                "tool.request",
                "action: tool\ntool: shell\ninput: false",
            )
            .unwrap();
        store
            .tool_outcome(
                &s,
                "work",
                request,
                "shell",
                "tool.result",
                "exit_code: 2\ncommand failed",
            )
            .unwrap();
        let before = store.history(&s).unwrap();
        let dump = Dump::load(&store, &s.id, None, false).unwrap();
        assert!(dump.markdown().unwrap().contains(&raw));
        assert!(!dump.preview().contains("LAST_RAW_LINE"));
        assert!(dump.diagnostics.iter().any(|d| d.contains("Legacy run")));
        assert!(dump
            .diagnostics
            .iter()
            .any(|d| d.contains("process exited with 2")));
        assert!(dump
            .diagnostics
            .iter()
            .any(|d| d.contains("protocol parser")));
        assert!(!dump
            .diagnostics
            .iter()
            .any(|d| d.contains("no validated outcome")));
        assert_eq!(store.history(&s).unwrap(), before);
        assert_eq!(store.session(&s.id).unwrap().status, "ready");
    }
    #[test]
    fn exact_messages_are_deduplicated_branch_scoped_and_missing_blobs_are_explicit() {
        let (_root, store, mut s) = fixture();
        let messages = vec![
            json!({"role":"system","content":"PROTOCOL"}),
            json!({"role":"user","content":"context"}),
        ];
        record_request(&store, &s, "work", "one", "coding", "fixture", &messages).unwrap();
        let mut call = crate::usage::Call::new(
            &s,
            "work",
            "coding",
            "fixture",
            "fixture",
            Default::default(),
            &messages,
        );
        call.id = "one".into();
        call.status = "completed".into();
        record_response(&store, &s, &call, 123, Some("visible answer")).unwrap();
        let cut = store
            .event(&s, Some("work"), "checkpoint", "fork here")
            .unwrap();
        let parent = s.active_branch.clone();
        record_request(
            &store,
            &s,
            "work",
            "parent-future",
            "coding",
            "fixture",
            &messages,
        )
        .unwrap();
        store.fork(&mut s, cut, "work").unwrap();
        let mut followup = messages.clone();
        followup.push(json!({"role":"assistant","content":"visible answer"}));
        followup.push(json!({"role":"user","content":"Tool error: disabled"}));
        record_request(&store, &s, "work", "two", "manager", "fixture", &followup).unwrap();
        let dump = Dump::load(&store, &s.id, None, false).unwrap();
        assert_eq!(dump.messages.len(), 4);
        let reconstruct = |event: &Event| {
            serde_json::from_str::<Value>(&event.data).unwrap()["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| dump.messages[v.as_str().unwrap()].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            reconstruct(
                dump.events
                    .iter()
                    .find(|e| e.kind == "model.request" && e.data.contains("two"))
                    .unwrap()
            ),
            followup
        );
        assert!(!dump.events.iter().any(|e| e.data.contains("parent-future")));
        assert!(Dump::load(&store, &s.id, Some(&parent), false)
            .unwrap()
            .events
            .iter()
            .any(|e| e.data.contains("parent-future")));
        assert!(Dump::load(&store, &s.id, None, true)
            .unwrap()
            .events
            .iter()
            .any(|e| e.data.contains("parent-future")));
        let other = store.create("demo", "other", "work").unwrap();
        assert!(Dump::load(&store, &s.id, Some(&other.active_branch), false).is_err());
        assert!(dump
            .diagnostics
            .iter()
            .any(|d| d.contains("two: no response")));
        let id = dump.messages.keys().next().unwrap();
        std::fs::remove_file(
            store
                .directory
                .join("blobs")
                .join(format!("trace-{id}.json")),
        )
        .unwrap();
        let partial = Dump::load(&store, &s.id, None, false).unwrap();
        assert!(partial
            .diagnostics
            .iter()
            .any(|d| d.contains("unavailable")));
        assert_eq!(partial.events.len(), dump.events.len());
        assert!(load_message(&store, "../../secret").is_err());
    }
    #[test]
    fn export_is_complete_and_never_overwrites() {
        let (root, store, s) = fixture();
        let text = format!("{}THE_END", "long ".repeat(2000));
        store
            .event(&s, Some("work"), "model.output", &text)
            .unwrap();
        let dump = Dump::load(&store, &s.id, None, false).unwrap();
        let path = root.path().join("dump.md");
        dump.save(&path, false).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains(&text));
        assert!(dump.save(&path, true).is_err());
        let json = root.path().join("dump.json");
        dump.save(&json, true).unwrap();
        let parsed: Value = serde_json::from_slice(&std::fs::read(json).unwrap()).unwrap();
        assert_eq!(parsed["events"][1]["data"], text);
        assert_eq!(store.history(&s).unwrap().len(), 2);
    }
    #[cfg(unix)]
    #[test]
    fn trace_blobs_reject_symlinks_and_corruption() {
        let (root, store, s) = fixture();
        let message = json!({"role":"user","content":"test"});
        let id = save_message(&store, &message).unwrap();
        let path = store
            .directory
            .join("blobs")
            .join(format!("trace-{id}.json"));
        std::fs::write(&path, "{}").unwrap();
        assert!(load_message(&store, &id).is_err());
        assert!(save_message(&store, &message).is_err());
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(root.path().join("absent"), &path).unwrap();
        assert!(
            record_request(&store, &s, "work", "bad", "coding", "fixture", &[message]).is_err()
        );
        assert!(!root.path().join("absent").exists());
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn oversized_trace_is_refused_instead_of_becoming_a_missing_message_diagnostic() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let message = json!({"role":"user","content":"x".repeat(4096)});
        let id = save_message(&store, &message).unwrap();
        record_request(
            &store,
            &session,
            "step",
            "call",
            "coding",
            "fixture",
            &[message],
        )
        .unwrap();
        store.limits.blob_bytes = 1024;
        assert!(load_message(&store, &id).is_err());
        assert!(Dump::load(&store, &session.id, None, false)
            .err()
            .unwrap()
            .to_string()
            .contains("Trace exceeds"));
        store.limits.blob_bytes = 8192;
        store.limits.export_bytes = 4096;
        assert!(Dump::load(&store, &session.id, None, false).is_err());
        assert!(store
            .directory
            .join("blobs")
            .join(format!("trace-{id}.json"))
            .exists());
    }
}
