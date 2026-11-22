use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::fs;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workflow: String,
    pub goal: String,
    pub active_branch: String,
    pub current_step: String,
    pub status: String,
}
#[derive(Debug, Clone)]
pub struct Branch {
    pub id: String,
    pub parent: Option<String>,
    pub fork_event: Option<i64>,
}
#[derive(Debug, Clone)]
pub struct FileChange {
    pub event_id: i64,
    pub step: String,
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}
#[derive(Debug, Clone)]
pub struct WorkspaceEdit {
    pub event_id: i64,
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Artifact {
    pub name: String,
    pub content: String,
    pub step: String,
    pub event_id: i64,
}

#[derive(Debug, Clone)]
pub struct ArtifactMetadata {
    pub name: String,
    pub step: String,
    pub event_id: i64,
    pub bytes: Option<u64>,
    pub(crate) blob: String,
}
#[derive(Debug, Clone)]
pub struct AttemptInfo {
    pub id: String,
    pub step: String,
    pub definition_event: i64,
    pub start_event: i64,
    pub end_event: Option<i64>,
    pub status: String,
}

type Event = (i64, String, String, String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History(std::sync::Arc<Vec<Event>>);
impl std::ops::Deref for History {
    type Target = Vec<Event>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for History {
    fn deref_mut(&mut self) -> &mut Self::Target {
        std::sync::Arc::make_mut(&mut self.0)
    }
}
impl Serialize for History {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}
impl IntoIterator for History {
    type Item = Event;
    type IntoIter = std::vec::IntoIter<Event>;
    fn into_iter(self) -> Self::IntoIter {
        std::sync::Arc::unwrap_or_clone(self.0).into_iter()
    }
}
impl<'a> IntoIterator for &'a History {
    type Item = &'a Event;
    type IntoIter = std::slice::Iter<'a, Event>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
type HistoryCache = (String, String, (u64, i64), History);
pub struct Store {
    pub(crate) active_project:
        std::cell::RefCell<std::sync::Weak<crate::project_execution::ProjectLease>>,
    pub(crate) limits: crate::resource_limits::Limits,
    #[cfg(test)]
    pub(crate) blob_reads: std::cell::Cell<usize>,
    history_cache: std::cell::RefCell<Option<HistoryCache>>,
    pub(crate) db: Connection,
    pub directory: PathBuf,
    pub(crate) directory_fd: crate::anchored_fs::Dir,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let (directory, directory_fd) = crate::paths::state_directory_handle(root)?;
        let db = crate::anchored_sqlite::open(&directory_fd)?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
           BEGIN;
           CREATE TABLE IF NOT EXISTS sessions (
             id TEXT PRIMARY KEY, workflow TEXT NOT NULL, goal TEXT NOT NULL,
             active_branch TEXT NOT NULL, current_step TEXT NOT NULL, status TEXT NOT NULL);
           CREATE TABLE IF NOT EXISTS branches (
             id TEXT PRIMARY KEY, session TEXT NOT NULL, parent TEXT, fork_event INTEGER,
             FOREIGN KEY(session) REFERENCES sessions(id));
           CREATE TABLE IF NOT EXISTS events (
             id INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT NOT NULL, branch TEXT NOT NULL,
             step TEXT, kind TEXT NOT NULL, data TEXT NOT NULL,
             timestamp TEXT NOT NULL DEFAULT (datetime('now')));
           CREATE TABLE IF NOT EXISTS artifacts (
             id INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT NOT NULL, branch TEXT NOT NULL,
             event_id INTEGER NOT NULL, step TEXT NOT NULL, name TEXT NOT NULL,
             blob_path TEXT NOT NULL);
           CREATE TABLE IF NOT EXISTS file_changes (
             event_id INTEGER PRIMARY KEY, session TEXT NOT NULL, branch TEXT NOT NULL,
             step TEXT NOT NULL, path TEXT NOT NULL, before_blob TEXT, after_blob TEXT NOT NULL);
           CREATE TABLE IF NOT EXISTS restore_changes (
             event_id INTEGER PRIMARY KEY, session TEXT NOT NULL, branch TEXT NOT NULL,
             path TEXT NOT NULL, before_blob TEXT, after_blob TEXT);
           CREATE TABLE IF NOT EXISTS write_intents (
             id TEXT PRIMARY KEY, session TEXT NOT NULL, branch TEXT NOT NULL,
             step TEXT NOT NULL, path TEXT NOT NULL, before_text TEXT, after_text TEXT NOT NULL,
             status TEXT NOT NULL CHECK(status IN ('pending','completed','cancelled')));
           CREATE TABLE IF NOT EXISTS execution_owners (
             session TEXT PRIMARY KEY REFERENCES sessions(id), token TEXT NOT NULL,
             pid INTEGER NOT NULL);
           CREATE TABLE IF NOT EXISTS project_owner (
             singleton INTEGER PRIMARY KEY CHECK(singleton=1), token TEXT NOT NULL UNIQUE,
             session TEXT NOT NULL REFERENCES sessions(id), branch TEXT NOT NULL,
             operation TEXT NOT NULL, pid INTEGER NOT NULL);
           CREATE TABLE IF NOT EXISTS step_attempts (
             id TEXT PRIMARY KEY, session TEXT NOT NULL, branch TEXT NOT NULL,
             step TEXT NOT NULL, definition_event INTEGER NOT NULL, start_event INTEGER NOT NULL,
             end_event INTEGER, status TEXT NOT NULL);
           CREATE TABLE IF NOT EXISTS manager_routes (
             message_id TEXT PRIMARY KEY, session TEXT NOT NULL REFERENCES sessions(id), branch TEXT NOT NULL,
             step TEXT NOT NULL, original TEXT NOT NULL, workflow_digest TEXT NOT NULL,
             decision_event INTEGER NOT NULL, state TEXT NOT NULL, attempt TEXT, call_id TEXT, owner_token TEXT, clarification_thread TEXT NOT NULL DEFAULT '[]');
           CREATE INDEX IF NOT EXISTS manager_routes_scope ON manager_routes(session,branch,decision_event);
           CREATE TABLE IF NOT EXISTS execution_steering (
             token TEXT PRIMARY KEY REFERENCES project_owner(token) ON DELETE CASCADE,
             session TEXT NOT NULL REFERENCES sessions(id), protocol INTEGER NOT NULL,
             step TEXT, attempt TEXT, workflow_digest TEXT, accepting INTEGER NOT NULL DEFAULT 0);
           CREATE TABLE IF NOT EXISTS model_calls (
             id TEXT PRIMARY KEY, session TEXT NOT NULL, branch TEXT NOT NULL,
             created_event INTEGER NOT NULL, payload TEXT NOT NULL);
           CREATE INDEX IF NOT EXISTS calls_session ON model_calls(session,created_event);
           CREATE INDEX IF NOT EXISTS restores_branch ON restore_changes(session,branch,event_id);
           CREATE INDEX IF NOT EXISTS changes_branch ON file_changes(session,branch,event_id);
           CREATE INDEX IF NOT EXISTS event_branch ON events(session,branch,id);
           CREATE INDEX IF NOT EXISTS artifact_branch ON artifacts(session,branch,event_id);
           COMMIT;",
        )?;
        // Upgrade existing private stores under a writer transaction so two
        // starting clients cannot race an ALTER after inspecting old columns.
        {
            let tx = rusqlite::Transaction::new_unchecked(
                &db,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            let columns = {
                let mut q = db.prepare("PRAGMA table_info(manager_routes)")?;
                let names = q.query_map([], |r| r.get::<_, String>(1))?;
                names.collect::<rusqlite::Result<Vec<_>>>()?
            };
            if !columns.iter().any(|n| n == "owner_token") {
                db.execute_batch("ALTER TABLE manager_routes ADD COLUMN owner_token TEXT;")?;
            }
            if !columns.iter().any(|n| n == "clarification_thread") {
                db.execute_batch("ALTER TABLE manager_routes ADD COLUMN clarification_thread TEXT NOT NULL DEFAULT '[]';")?;
            }
            tx.commit()?;
        }
        Ok(Self {
            active_project: std::cell::RefCell::new(std::sync::Weak::new()),
            #[cfg(test)]
            blob_reads: std::cell::Cell::new(0),
            limits: crate::config::load_config(root)
                .map(|c| c.limits)
                .unwrap_or_default(),
            history_cache: std::cell::RefCell::new(None),
            db,
            directory,
            directory_fd,
        })
    }
    pub(crate) fn read_blob(&self, name: &str) -> Result<String> {
        #[cfg(test)]
        self.blob_reads.set(self.blob_reads.get() + 1);
        self.directory_fd
            .open_dir(Path::new("blobs"))?
            .read_string(Path::new(name), self.limits.blob_bytes as u64)
    }
    pub(crate) fn read_blob_budget(
        &self,
        name: &str,
        budget: &mut crate::resource_limits::Budget,
    ) -> Result<String> {
        #[cfg(test)]
        self.blob_reads.set(self.blob_reads.get() + 1);
        let text = self
            .directory_fd
            .open_dir(Path::new("blobs"))?
            .read_string(
                Path::new(name),
                self.limits.blob_bytes.min(budget.remaining()) as u64,
            )?;
        budget.charge(text.len())?;
        Ok(text)
    }
    fn write_blob(&self, name: &str, text: &str) -> Result<()> {
        let dir = self.directory_fd.create_dirs(Path::new("blobs"))?;
        let mut file = dir.create_new(Path::new(name))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        dir.sync()
    }
    pub fn create(&self, workflow: &str, goal: &str, start: &str) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let branch = Uuid::new_v4().to_string();
        let session = Session {
            id: id.clone(),
            workflow: workflow.into(),
            goal: goal.into(),
            active_branch: branch.clone(),
            current_step: start.into(),
            status: "ready".into(),
        };
        let transaction = self.db.unchecked_transaction()?;
        self.db.execute(
            "INSERT INTO sessions VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, workflow, goal, branch, start, "ready"],
        )?;
        self.db.execute(
            "INSERT INTO branches(id,session,parent,fork_event) VALUES (?1,?2,NULL,NULL)",
            params![branch, id],
        )?;
        self.event(&session, Some(start), "session.started", goal)?;
        transaction.commit()?;
        Ok(session)
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        self.db.query_row("SELECT id, workflow, goal, active_branch, current_step, status FROM sessions WHERE id=?1", [id], |r| Ok(Session { id:r.get(0)?, workflow:r.get(1)?, goal:r.get(2)?, active_branch:r.get(3)?, current_step:r.get(4)?, status:r.get(5)? })).map_err(Into::into)
    }
    pub fn list(&self) -> Result<Vec<Session>> {
        let mut st = self.db.prepare("SELECT id, workflow, goal, active_branch, current_step, status FROM sessions ORDER BY rowid DESC")?;
        let rows = st.query_map([], |r| {
            Ok(Session {
                id: r.get(0)?,
                workflow: r.get(1)?,
                goal: r.get(2)?,
                active_branch: r.get(3)?,
                current_step: r.get(4)?,
                status: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn save(&self, s: &Session) -> Result<()> {
        self.db.execute(
            "UPDATE sessions SET active_branch=?2,current_step=?3,status=?4 WHERE id=?1",
            params![s.id, s.active_branch, s.current_step, s.status],
        )?;
        Ok(())
    }
    pub fn event(&self, s: &Session, step: Option<&str>, kind: &str, data: &str) -> Result<i64> {
        let tx = if self.db.is_autocommit() {
            Some(self.db.unchecked_transaction()?)
        } else {
            None
        };
        self.db.execute(
            "INSERT INTO events(session,branch,step,kind,data) VALUES (?1,?2,?3,?4,?5)",
            params![s.id, s.active_branch, step, kind, data],
        )?;
        let event = self.db.last_insert_rowid();
        let status = match kind {
            "step.completed" => Some("completed"),
            "step.deviation" => Some("deviation"),
            "step.blocked" => Some("blocked"),
            "runner.error" | "runner.recovered" => Some("interrupted"),
            _ => None,
        };
        if let Some(status) = status {
            self.db.execute("UPDATE step_attempts SET end_event=?1,status=?2 WHERE session=?3 AND branch=?4 AND status='running' AND (?5 IS NULL OR step=?5)",
                params![event,status,s.id,s.active_branch,step])?;
        }
        if let Some(tx) = tx {
            tx.commit()?;
        }
        Ok(event)
    }
    pub fn branch(&self, id: &str) -> Result<Branch> {
        self.db
            .query_row(
                "SELECT id,parent,fork_event FROM branches WHERE id=?1",
                [id],
                |r| {
                    Ok(Branch {
                        id: r.get(0)?,
                        parent: r.get(1)?,
                        fork_event: r.get(2)?,
                    })
                },
            )
            .map_err(Into::into)
    }
    pub fn branches(&self, session: &str) -> Result<Vec<Branch>> {
        let mut st = self
            .db
            .prepare("SELECT id,parent,fork_event FROM branches WHERE session=?1 ORDER BY rowid")?;
        let rows = st.query_map([session], |r| {
            Ok(Branch {
                id: r.get(0)?,
                parent: r.get(1)?,
                fork_event: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn switch_branch(&self, s: &mut Session, branch: &str, step: &str) -> Result<()> {
        let previous = s.clone();
        self.with_project_control(&previous, "branch-control", |lease| {
            self.check_project(lease, s)?;
            self.ensure_no_pending_writes()?;
            let found: i64 = self.db.query_row(
                "SELECT count(*) FROM branches WHERE session=?1 AND id=?2",
                params![s.id, branch],
                |r| r.get(0),
            )?;
            if found != 1 {
                bail!("branch does not belong to this session");
            }
            let transaction = self.db.unchecked_transaction()?;
            self.invalidate_manager_routes(
                s,
                false,
                "Branch selection invalidated the reservation; no cross-branch delivery",
            )?;
            let mut next = s.clone();
            next.active_branch = branch.into();
            next.current_step = step.into();
            next.status = "ready".into();
            self.save(&next)?;
            self.event(
                &next,
                Some(step),
                "branch.selected",
                "manual selection; filesystem unchanged",
            )?;
            transaction.commit()?;
            *s = next;
            Ok(())
        })
    }
    pub fn fork(&self, s: &mut Session, event_id: i64, step: &str) -> Result<()> {
        let previous = s.clone();
        self.with_project_control(&previous, "branch-control", |lease| {
            self.fork_owned(s, event_id, step, lease)
        })
    }
    pub(crate) fn fork_owned(
        &self,
        s: &mut Session,
        event_id: i64,
        step: &str,
        lease: &crate::project_execution::ProjectLease,
    ) -> Result<()> {
        self.check_project(lease, s)?;
        self.ensure_no_pending_writes()?;
        let valid: bool = self.history(s)?.iter().any(|(id, _, _, _)| *id == event_id);
        if !valid {
            bail!("fork event is not part of current branch ancestry");
        }
        let transaction = if self.db.is_autocommit() {
            Some(self.db.unchecked_transaction()?)
        } else {
            None
        };
        self.invalidate_manager_routes(
            s,
            false,
            "Fork invalidated the reservation; no cross-branch replay",
        )?;
        let branch = Uuid::new_v4().to_string();
        self.db.execute(
            "INSERT INTO branches(id,session,parent,fork_event) VALUES (?1,?2,?3,?4)",
            params![branch, s.id, s.active_branch, event_id],
        )?;
        let mut next = s.clone();
        next.active_branch = branch;
        next.current_step = step.into();
        next.status = "ready".into();
        self.save(&next)?;
        self.event(
            &next,
            Some(step),
            "branch.forked",
            &format!("from_event: {event_id}"),
        )?;
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        *s = next;
        Ok(())
    }
    pub(crate) fn lineage(&self, branch: &str) -> Result<Vec<(String, i64)>> {
        let mut line = vec![];
        let mut cursor = branch.to_string();
        let mut cutoff = i64::MAX;
        let mut visited = std::collections::HashSet::new();
        let owner: String = self.db.query_row(
            "SELECT session FROM branches WHERE id=?1",
            [branch],
            |row| row.get(0),
        )?;
        loop {
            if !visited.insert(cursor.clone()) {
                bail!("corrupt branch ancestry: cycle");
            }
            let session: String = self.db.query_row(
                "SELECT session FROM branches WHERE id=?1",
                [&cursor],
                |row| row.get(0),
            )?;
            if session != owner {
                bail!("corrupt branch ancestry: cross-session parent");
            }
            if line.len() >= self.limits.history_events {
                bail!(
                    "Branch ancestry exceeds limits.history_events; increase the limit and restart"
                );
            }
            line.push((cursor.clone(), cutoff));
            let b = self.branch(&cursor)?;
            match (b.parent, b.fork_event) {
                (Some(parent), Some(fork)) => {
                    cursor = parent;
                    cutoff = cutoff.min(fork);
                }
                (None, None) => break,
                _ => bail!("corrupt branch ancestry"),
            }
        }
        line.reverse();
        Ok(line)
    }
    pub(crate) fn data_revision(&self) -> Result<(u64, i64)> {
        Ok((
            self.db.total_changes(),
            self.db.query_row("PRAGMA data_version", [], |r| r.get(0))?,
        ))
    }
    pub fn history(&self, s: &Session) -> Result<History> {
        let cacheable = self.db.is_autocommit();
        let revision = self.data_revision()?;
        if cacheable {
            if let Some((session, branch, version, history)) = self.history_cache.borrow().as_ref()
            {
                if session == &s.id && branch == &s.active_branch && version == &revision {
                    return Ok(history.clone());
                }
            }
        }
        // One read snapshot covers both preflight and payloads. Never cache transaction-local rows.
        let tx = if cacheable {
            Some(self.db.unchecked_transaction()?)
        } else {
            None
        };
        // octet_length(column) reads SQLite's record header, avoiding an oversized
        // text allocation just to discover that its payload is over budget.
        let mut result = vec![];
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let (count, bytes, largest): (usize, usize, usize) = self.db.query_row(
                "SELECT count(*),coalesce(sum(octet_length(data)+coalesce(octet_length(step),0)+octet_length(kind)+96),0),coalesce(max(octet_length(data)),0) FROM events WHERE session=?1 AND branch=?2 AND id<=?3",
                params![s.id, branch, cutoff], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            if count > self.limits.history_events.saturating_sub(result.len())
                || largest > self.limits.blob_bytes
            {
                return Err(crate::resource_limits::Exceeded("History exceeds limits.history_events/blob_bytes; recorded data is unchanged. Increase the limit and restart to inspect it in full.".into()).into());
            }
            budget.charge(bytes)?;
            let mut st = self.db.prepare("SELECT id,coalesce(step,''),kind,data FROM events WHERE session=?1 AND branch=?2 AND id<=?3 ORDER BY id")?;
            let rows = st.query_map(params![s.id, branch, cutoff], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            result.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        if let Some(tx) = tx {
            tx.commit()?;
        }
        let history = History(std::sync::Arc::new(result));
        if cacheable && self.data_revision()? == revision {
            *self.history_cache.borrow_mut() = Some((
                s.id.clone(),
                s.active_branch.clone(),
                revision,
                history.clone(),
            ));
        }
        Ok(history)
    }
    /// Bounded diagnostic window; these previews never drive workflow state or export as full bodies.
    pub fn history_previews(&self, s: &Session) -> Result<Vec<Event>> {
        let mut events = Vec::new();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut query = self.db.prepare("SELECT id,substr(coalesce(step,''),1,80),substr(kind,1,80),CASE WHEN octet_length(data)>?4 THEN '[Body exceeds limits.blob_bytes; raise the limit and restart]' ELSE substr(data,1,512) END,octet_length(data) FROM events WHERE session=?1 AND branch=?2 AND id<=?3 ORDER BY id DESC LIMIT 2000")?;
            for row in
                query.query_map(params![s.id, branch, cutoff, self.limits.blob_bytes], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, u64>(4)?,
                    ))
                })?
            {
                let (id, step, kind, text, bytes) = row?;
                events.push((id,step,kind,format!("Preview only · {bytes} recorded bytes · Enter loads the exact body\n\n{text}")));
            }
            events.sort_by_key(|e| e.0);
            if events.len() > 2000 {
                events.drain(..events.len() - 2000);
            }
        }
        Ok(events)
    }
    pub fn event_body(&self, s: &Session, id: i64) -> Result<String> {
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            if id > cutoff {
                continue;
            }
            let bytes: Option<usize> = self.db.query_row("SELECT octet_length(data) FROM events WHERE session=?1 AND branch=?2 AND id=?3",params![s.id,branch,id],|r|r.get(0)).optional()?;
            if let Some(bytes) = bytes {
                if bytes > self.limits.blob_bytes {
                    bail!("Event body exceeds limits.blob_bytes; saved evidence is unchanged. Increase the limit and restart.");
                }
                return self
                    .db
                    .query_row(
                        "SELECT data FROM events WHERE session=?1 AND branch=?2 AND id=?3 AND octet_length(data)<=?4",
                        params![s.id, branch, id, self.limits.blob_bytes],
                        |r| r.get(0),
                    )
                    .map_err(Into::into);
            }
        }
        bail!("Event is not visible on the selected branch")
    }
    pub fn add_artifact(&self, s: &Session, step: &str, name: &str, content: &str) -> Result<()> {
        let path = Path::new(name);
        if name.is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            bail!("artifact path must be relative, without ..");
        }
        let blob = format!("artifact-{}.txt", Uuid::new_v4());
        let dir = self.directory_fd.create_dirs(Path::new("blobs"))?;
        self.directory_fd.sync()?;
        let blob_path = self.directory.join("blobs").join(&blob);
        let mut file = dir.create_new(Path::new(&blob))?;
        let publication: Result<()> = (|| {
            // Make the immutable body durable before publishing references.
            // A crash here may leave an unreferenced blob, never a success event.
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            dir.sync()?;
            let tx = self.db.unchecked_transaction()?;
            tx.execute("INSERT INTO events(session,branch,step,kind,data) VALUES (?1,?2,?3,'artifact.created',?4)",
                params![s.id,s.active_branch,step,name])?;
            let ev = tx.last_insert_rowid();
            tx.execute("INSERT INTO artifacts(session,branch,event_id,step,name,blob_path) VALUES (?1,?2,?3,?4,?5,?6)",
                params![s.id,s.active_branch,ev,step,name,blob])?;
            tx.commit()?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = publication {
            if let Err(cleanup) = dir.target(Path::new(&blob))?.unlink() {
                return Err(error.context(format!(
                    "artifact publication failed; could not remove unpublished blob {}: {cleanup}",
                    blob_path.display()
                )));
            }
            return Err(error.context("artifact publication failed"));
        }
        Ok(())
    }
    pub fn add_change(
        &self,
        s: &Session,
        step: &str,
        path: &str,
        before: Option<&str>,
        after: &str,
    ) -> Result<i64> {
        let before_name = before.map(|_| format!("edit-before-{}.txt", Uuid::new_v4()));
        let after_name = format!("edit-after-{}.txt", Uuid::new_v4());
        if let (Some(content), Some(name)) = (before, &before_name) {
            self.write_blob(name, content)?;
        }
        self.write_blob(&after_name, after)?;
        let event_id = self.event(s, Some(step), "file.changed", path)?;
        self.db.execute(
            "INSERT INTO file_changes(event_id,session,branch,step,path,before_blob,after_blob) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![event_id, s.id, s.active_branch, step, path, before_name, after_name],
        )?;
        Ok(event_id)
    }

    pub fn changes(&self, s: &Session) -> Result<Vec<FileChange>> {
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        let mut result = Vec::new();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut stmt = self.db.prepare(
                "SELECT event_id,step,path,before_blob,after_blob FROM file_changes WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id"
            )?;
            let rows = stmt.query_map(params![s.id, branch, cutoff], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (event_id, step, path, before_blob, after_blob) = row?;
                if result.len() >= self.limits.history_events {
                    bail!("Edit collection exceeds limits.history_events");
                }
                budget.charge(path.len() + step.len() + 128)?;
                let before = before_blob
                    .map(|b| self.read_blob_budget(&b, &mut budget))
                    .transpose()?;
                let after = self.read_blob_budget(&after_blob, &mut budget)?;
                result.push(FileChange {
                    event_id,
                    step,
                    path,
                    before,
                    after,
                });
            }
        }
        Ok(result)
    }
    /// All native writes and explicit restores visible on this branch, in event order.
    /// Restores are kept separate because they may remove a file entirely.
    pub fn workspace_edits(&self, s: &Session) -> Result<Vec<WorkspaceEdit>> {
        let mut edits: Vec<WorkspaceEdit> = self
            .changes(s)?
            .into_iter()
            .map(|change| WorkspaceEdit {
                event_id: change.event_id,
                path: change.path,
                before: change.before,
                after: Some(change.after),
            })
            .collect();
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        for edit in &edits {
            budget.charge(
                edit.before.as_ref().map_or(0, String::len)
                    + edit.after.as_ref().map_or(0, String::len),
            )?;
        }
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut stmt = self.db.prepare(
                "SELECT event_id,path,before_blob,after_blob
                 FROM restore_changes WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id"
            )?;
            let rows = stmt.query_map(params![s.id, branch, cutoff], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            for row in rows {
                let (event_id, path, before_blob, after_blob) = row?;
                if edits.len() >= self.limits.history_events {
                    bail!("Restore collection exceeds limits.history_events");
                }
                budget.charge(path.len() + 128)?;
                let before = before_blob
                    .map(|blob| self.read_blob_budget(&blob, &mut budget))
                    .transpose()?;
                let after = after_blob
                    .map(|blob| self.read_blob_budget(&blob, &mut budget))
                    .transpose()?;
                edits.push(WorkspaceEdit {
                    event_id,
                    path,
                    before,
                    after,
                });
            }
        }
        edits.sort_by_key(|edit| edit.event_id);
        Ok(edits)
    }

    /// Journal one already-applied workspace restore, including file deletions.
    pub fn add_restoration(
        &self,
        s: &Session,
        path: &str,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<i64> {
        let tx = self.db.unchecked_transaction()?;
        let save = |content: Option<&str>| -> Result<Option<String>> {
            match content {
                Some(text) => {
                    let name = format!("restore-{}.txt", Uuid::new_v4());
                    self.write_blob(&name, text)?;
                    Ok(Some(name))
                }
                None => Ok(None),
            }
        };
        let before_blob = save(before)?;
        let after_blob = save(after)?;
        let event_id = self.event(s, None, "workspace.restored", path)?;
        self.db.execute(
            "INSERT INTO restore_changes(event_id,session,branch,path,before_blob,after_blob)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                event_id,
                s.id,
                s.active_branch,
                path,
                before_blob,
                after_blob
            ],
        )?;
        tx.commit()?;
        Ok(event_id)
    }

    pub fn current_run_artifacts(&self, s: &Session, step: &str) -> Result<Vec<String>> {
        Ok(self
            .attempt_artifacts(s, step)?
            .into_iter()
            .map(|a| a.name)
            .collect())
    }
    pub fn begin_attempt(
        &self,
        s: &Session,
        step: &str,
        definition: &str,
        model: &str,
    ) -> Result<String> {
        let tx = if self.db.is_autocommit() {
            Some(self.db.unchecked_transaction()?)
        } else {
            None
        };
        let definition_event = self.event(s, Some(step), "step.definition", definition)?;
        let id = Uuid::new_v4().to_string();
        let start = self.event(
            s,
            Some(step),
            "step.started",
            &format!("model: {model}\nattempt: {id}"),
        )?;
        self.db.execute("UPDATE step_attempts SET status='interrupted',end_event=?2 WHERE session=?1 AND status='running'", params![s.id,start-1])?;
        self.db.execute("INSERT INTO step_attempts(id,session,branch,step,definition_event,start_event,status) VALUES (?1,?2,?3,?4,?5,?6,'running')",
            params![id,s.id,s.active_branch,step,definition_event,start])?;
        if let Some(tx) = tx {
            tx.commit()?;
        }
        Ok(id)
    }
    pub fn latest_attempt(&self, s: &Session, step: &str) -> Result<(String, i64)> {
        self.db.query_row("SELECT id,start_event FROM step_attempts WHERE session=?1 AND branch=?2 AND step=?3 ORDER BY start_event DESC LIMIT 1",
            params![s.id,s.active_branch,step], |row|Ok((row.get(0)?,row.get(1)?))).map_err(Into::into)
    }
    pub fn active_attempt(&self, s: &Session, step: &str) -> Result<(String, i64)> {
        self.db.query_row("SELECT id,start_event FROM step_attempts WHERE session=?1 AND branch=?2 AND step=?3 AND status='running' ORDER BY start_event DESC LIMIT 1",
            params![s.id,s.active_branch,step], |r| Ok((r.get(0)?,r.get(1)?))).map_err(Into::into)
    }
    pub fn attempt_artifacts(&self, s: &Session, step: &str) -> Result<Vec<Artifact>> {
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        let (_, start) = self.active_attempt(s, step)?;
        let mut query = self.db.prepare("SELECT name,blob_path,event_id FROM artifacts WHERE session=?1 AND branch=?2 AND step=?3 AND event_id>?4 ORDER BY event_id")?;
        let rows = query.query_map(params![s.id, s.active_branch, step, start], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut versions = std::collections::BTreeMap::new();
        for row in rows {
            let (name, blob, event_id) = row?;
            if versions.len() >= self.limits.history_events && !versions.contains_key(&name) {
                bail!("Artifact collection exceeds limits.history_events");
            }
            budget.charge(name.len() + blob.len() + 128)?;
            versions.insert(
                name.clone(),
                Artifact {
                    name,
                    content: self.read_blob_budget(&blob, &mut budget)?,
                    step: step.into(),
                    event_id,
                },
            );
        }
        Ok(versions.into_values().collect())
    }
    pub fn attempts(&self, s: &Session) -> Result<Vec<AttemptInfo>> {
        let mut result = Vec::new();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut query = self.db.prepare("SELECT id,step,definition_event,start_event,end_event,status FROM step_attempts WHERE session=?1 AND branch=?2 AND start_event<=?3 ORDER BY start_event")?;
            let rows = query.query_map(params![s.id, branch, cutoff], |r| {
                Ok(AttemptInfo {
                    id: r.get(0)?,
                    step: r.get(1)?,
                    definition_event: r.get(2)?,
                    start_event: r.get(3)?,
                    end_event: r.get(4)?,
                    status: r.get(5)?,
                })
            })?;
            for row in rows {
                let mut attempt = row?;
                if result.len() >= self.limits.history_events {
                    bail!("Attempt count exceeds limits.history_events");
                }
                if attempt.end_event.is_none_or(|end| end > cutoff) && cutoff != i64::MAX {
                    attempt.status = "inherited-partial".into();
                }
                attempt.end_event = attempt.end_event.filter(|end| *end <= cutoff);
                result.push(attempt);
            }
        }
        Ok(result)
    }
    pub fn attempt_history(&self, s: &Session) -> Result<Vec<String>> {
        Ok(self
            .attempts(s)?
            .into_iter()
            .map(|a| {
                format!(
                    "{}  {}  {}  events {}..{}",
                    a.id,
                    a.step,
                    a.status,
                    a.start_event,
                    a.end_event
                        .map(|end| end.to_string())
                        .unwrap_or_else(|| "open".into())
                )
            })
            .collect())
    }
    pub fn artifact_metadata(&self, s: &Session, source: &str) -> Result<Vec<ArtifactMetadata>> {
        // Choose visible versions before opening blobs: a missing historical body
        // must not prevent inspection of a healthy latest version.
        let mut metadata_budget =
            crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        let mut count = 0;
        let mut versions = std::collections::BTreeMap::new();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let (records, bytes): (usize,usize) = self.db.query_row("SELECT count(*),coalesce(sum(octet_length(step)+octet_length(name)+octet_length(blob_path)+128),0) FROM artifacts WHERE session=?1 AND branch=?2 AND event_id<=?3",params![s.id,branch,cutoff],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if records > self.limits.history_events.saturating_sub(count) {
                bail!("Artifact metadata exceeds limits.history_events");
            }
            count += records;
            metadata_budget.charge(bytes)?;
            let mut st=self.db.prepare("SELECT step,name,blob_path,event_id FROM artifacts WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id")?;
            let rows = st.query_map(params![s.id, branch, cutoff], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (step, name, blob, event) = row?;
                if source == "session" || source == step {
                    versions.insert(name, (step, blob, event));
                }
            }
        }
        if versions.len() > self.limits.history_events {
            bail!("Artifact count exceeds limits.history_events");
        }
        versions
            .into_iter()
            .map(|(name, (step, blob, event_id))| {
                let bytes = self
                    .directory_fd
                    .open_dir(Path::new("blobs"))
                    .and_then(|dir| dir.open_file(Path::new(&blob)))
                    .and_then(|file| Ok(file.metadata()?.len()))
                    .ok();
                Ok(ArtifactMetadata {
                    name,
                    step,
                    event_id,
                    bytes,
                    blob,
                })
            })
            .collect()
    }
    pub fn artifacts(&self, s: &Session, source: &str) -> Result<Vec<Artifact>> {
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        self.artifact_metadata(s, source)?
            .into_iter()
            .map(|meta| {
                let content = self.read_blob_budget(&meta.blob, &mut budget)?;
                Ok(Artifact {
                    name: meta.name,
                    step: meta.step,
                    event_id: meta.event_id,
                    content,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attempts_keep_versions_separate_across_retries_and_reopening() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let first = store
            .begin_attempt(&session, "step", "first definition", "fixture")
            .unwrap();
        store
            .add_artifact(&session, "step", "report.md", "old success")
            .unwrap();
        store
            .event(&session, Some("step"), "step.blocked", "retry")
            .unwrap();
        let second = store
            .begin_attempt(&session, "step", "second definition", "fixture")
            .unwrap();
        assert_ne!(first, second);
        assert!(store
            .current_run_artifacts(&session, "step")
            .unwrap()
            .is_empty());
        store
            .add_artifact(&session, "step", "report.md", "current failure")
            .unwrap();
        let version = store.attempt_artifacts(&session, "step").unwrap().remove(0);
        assert_eq!(version.content, "current failure");
        assert!(version.event_id > store.active_attempt(&session, "step").unwrap().1);
        drop(store);
        let reopened = Store::open(root.path()).unwrap();
        assert_eq!(reopened.active_attempt(&session, "step").unwrap().0, second);
        let attempts = reopened.attempt_history(&session).unwrap();
        assert_eq!(attempts.len(), 2);
        assert!(attempts[0].contains(&first) && attempts[0].contains("blocked"));
        assert!(attempts[1].contains(&second) && attempts[1].contains("running"));
    }

    #[test]
    fn forked_attempt_history_honors_cutoffs_and_recovery_closes_interrupted_attempts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("demo", "goal", "step").unwrap();
        let owner = store.claim_execution(&session.id, None).unwrap();
        let parent = store
            .begin_attempt(&session, "step", "definition", "fixture")
            .unwrap();
        let cutoff = store.active_attempt(&session, "step").unwrap().1;
        drop(owner);
        store.recover_session(&session.id).unwrap();
        assert!(store.attempt_history(&session).unwrap()[0].contains("interrupted"));
        store.fork(&mut session, cutoff, "step").unwrap();
        let child = store
            .begin_attempt(&session, "step", "child definition", "fixture")
            .unwrap();
        let history = store.attempt_history(&session).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history[0].contains(&parent) && history[0].contains("inherited-partial"));
        assert!(history[1].contains(&child));
        assert!(store
            .current_run_artifacts(&session, "step")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn attempt_creation_failure_rolls_back_its_definition_and_start() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let before = store.history(&session).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_attempt BEFORE INSERT ON step_attempts BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        assert!(store
            .begin_attempt(&session, "step", "definition", "fixture")
            .is_err());
        assert_eq!(store.history(&session).unwrap(), before);
    }
    #[test]
    fn artifact_blob_failure_does_not_publish_a_success_event() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "plan").unwrap();
        let before = store.history(&session).unwrap();
        // A file in place of the blob directory deterministically fails I/O.
        fs::write(store.directory.join("blobs"), "obstruction").unwrap();
        assert!(store
            .add_artifact(&session, "plan", "plan.md", "draft")
            .is_err());
        assert_eq!(store.history(&session).unwrap(), before);
        assert!(store.artifacts(&session, "session").unwrap().is_empty());
        fs::remove_file(store.directory.join("blobs")).unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "retry")
            .unwrap();
        assert_eq!(
            store.artifacts(&session, "session").unwrap()[0].content,
            "retry"
        );
    }

    #[test]
    fn artifact_database_failure_rolls_back_publication_and_preserves_previous_version() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "plan").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "original")
            .unwrap();
        let before = store.history(&session).unwrap();
        store
            .db
            .execute_batch(
                "CREATE TRIGGER reject_artifact BEFORE INSERT ON artifacts
            BEGIN SELECT RAISE(ABORT, 'injected artifact insert failure'); END;",
            )
            .unwrap();
        assert!(store
            .add_artifact(&session, "plan", "plan.md", "replacement")
            .is_err());
        assert_eq!(store.history(&session).unwrap(), before);
        assert_eq!(
            store.artifacts(&session, "session").unwrap()[0].content,
            "original"
        );
        assert_eq!(
            fs::read_dir(store.directory.join("blobs")).unwrap().count(),
            1
        );
        store
            .db
            .execute_batch("DROP TRIGGER reject_artifact;")
            .unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "retry")
            .unwrap();
        drop(store);
        let reopened = Store::open(root.path()).unwrap();
        assert_eq!(
            reopened.artifacts(&session, "session").unwrap()[0].content,
            "retry"
        );
        assert_eq!(
            reopened
                .history(&session)
                .unwrap()
                .iter()
                .filter(|(_, _, kind, _)| kind == "artifact.created")
                .count(),
            2
        );
    }

    #[test]
    fn artifact_commit_failure_rolls_back_event_and_blob() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "plan").unwrap();
        let before = store.history(&session).unwrap();
        // Defer a foreign-key violation so both INSERTs succeed and COMMIT fails.
        store
            .db
            .execute_batch(
                "PRAGMA defer_foreign_keys=ON;
            CREATE TRIGGER fail_commit BEFORE INSERT ON artifacts BEGIN
              INSERT INTO branches(id,session) VALUES ('injected-branch','missing-session');
            END;",
            )
            .unwrap();
        let error = store
            .add_artifact(&session, "plan", "plan.md", "draft")
            .unwrap_err();
        assert!(format!("{error:#}").contains("FOREIGN KEY constraint failed"));
        assert_eq!(store.history(&session).unwrap(), before);
        assert!(store.artifacts(&session, "session").unwrap().is_empty());
        assert_eq!(
            fs::read_dir(store.directory.join("blobs")).unwrap().count(),
            0
        );
        store.db.execute_batch("DROP TRIGGER fail_commit;").unwrap();
        store
            .add_artifact(&session, "plan", "plan.md", "retry")
            .unwrap();
        assert_eq!(
            store.artifacts(&session, "session").unwrap()[0].content,
            "retry"
        );
    }

    #[test]
    fn old_state_migrates_with_session_and_artifacts_intact() {
        let root = tempfile::tempdir().unwrap();
        let session_id;
        {
            let store = Store::open(root.path()).unwrap();
            let session = store.create("demo", "original goal", "plan").unwrap();
            store
                .add_artifact(&session, "plan", "plan.md", "original artifact")
                .unwrap();
            session_id = session.id;
        }
        let old_dir = root.path().join(".stepwright");
        fs::create_dir(&old_dir).unwrap();
        fs::rename(root.path().join(".greenlight/state"), old_dir.join("state")).unwrap();

        let reopened = Store::open(root.path()).unwrap();
        let session = reopened.session(&session_id).unwrap();
        assert_eq!(session.goal, "original goal");
        assert_eq!(
            reopened.artifacts(&session, "session").unwrap()[0].content,
            "original artifact"
        );
        assert!(!old_dir.join("state").exists());
        assert!(root.path().join(".greenlight/state/history.db").is_file());
    }

    #[test]
    fn fork_preserves_history_and_artifacts() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        let mut s = store.create("test", "goal", "a").unwrap();
        store.add_artifact(&s, "a", "plan.md", "old").unwrap();
        let mark = store.history(&s).unwrap().last().unwrap().0;
        store
            .add_artifact(&s, "a", "plan.md", "should disappear")
            .unwrap();
        store.fork(&mut s, mark, "a").unwrap();
        assert_eq!(store.artifacts(&s, "session").unwrap()[0].content, "old");
        assert!(!store
            .history(&s)
            .unwrap()
            .iter()
            .any(|(_, _, _, d)| d == "should disappear"));
    }
    #[test]
    fn corrupt_branch_ancestry_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let first = store.create("demo", "one", "plan").unwrap();
        let second = store.create("demo", "two", "plan").unwrap();
        store
            .db
            .execute(
                "UPDATE branches SET parent=id,fork_event=1 WHERE id=?1",
                [&first.active_branch],
            )
            .unwrap();
        assert!(store
            .history(&first)
            .unwrap_err()
            .to_string()
            .contains("cycle"));
        store
            .db
            .execute(
                "UPDATE branches SET parent=?1 WHERE id=?2",
                params![second.active_branch, first.active_branch],
            )
            .unwrap();
        assert!(store
            .history(&first)
            .unwrap_err()
            .to_string()
            .contains("cross-session"));
    }
    #[test]
    fn failed_session_publication_rolls_back_all_records() {
        for commit_failure in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let fault = if commit_failure {
                "PRAGMA defer_foreign_keys=ON;
                 CREATE TRIGGER fail_start BEFORE INSERT ON events WHEN NEW.kind='session.started'
                 BEGIN INSERT INTO branches(id,session) VALUES ('bad','missing'); END;"
            } else {
                "CREATE TRIGGER fail_start BEFORE INSERT ON events WHEN NEW.kind='session.started'
                 BEGIN SELECT RAISE(ABORT,'injected start failure'); END;"
            };
            store.db.execute_batch(fault).unwrap();
            assert!(store.create("demo", "goal", "work").is_err());
            assert!(store.list().unwrap().is_empty());
            for table in ["branches", "events"] {
                let count: i64 = store
                    .db
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, 0);
            }
            store.db.execute_batch("DROP TRIGGER fail_start").unwrap();
            assert!(store.create("demo", "retry", "work").is_ok());
        }
    }

    #[test]
    fn failed_branch_publication_preserves_database_and_in_memory_session() {
        for switching in [false, true] {
            for commit_failure in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let store = Store::open(root.path()).unwrap();
                let mut session = store.create("demo", "goal", "work").unwrap();
                let original = session.active_branch.clone();
                let mark = store.history(&session).unwrap().last().unwrap().0;
                if switching {
                    store.fork(&mut session, mark, "next").unwrap();
                }
                let before = serde_json::to_value(&session).unwrap();
                let history = store.history(&session).unwrap();
                let branches = store.branches(&session.id).unwrap().len();
                let kind = if switching {
                    "branch.selected"
                } else {
                    "branch.forked"
                };
                let body = if commit_failure {
                    "INSERT INTO branches(id,session) VALUES ('bad','missing');"
                } else {
                    "SELECT RAISE(ABORT,'injected branch failure');"
                };
                store
                    .db
                    .execute_batch(&format!(
                        "PRAGMA defer_foreign_keys=ON;
                    CREATE TRIGGER fail_branch BEFORE INSERT ON events WHEN NEW.kind='{kind}'
                    BEGIN {body} END;"
                    ))
                    .unwrap();
                let result = if switching {
                    store.switch_branch(&mut session, &original, "changed")
                } else {
                    store.fork(&mut session, mark, "changed")
                };
                assert!(result.is_err());
                assert_eq!(serde_json::to_value(&session).unwrap(), before);
                assert_eq!(
                    serde_json::to_value(store.session(&session.id).unwrap()).unwrap(),
                    before
                );
                assert_eq!(store.history(&session).unwrap(), history);
                assert_eq!(store.branches(&session.id).unwrap().len(), branches);
                store.db.execute_batch("DROP TRIGGER fail_branch").unwrap();
                if switching {
                    store
                        .switch_branch(&mut session, &original, "retry")
                        .unwrap();
                } else {
                    store.fork(&mut session, mark, "retry").unwrap();
                }
            }
        }
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    #[test]
    fn state_parent_swap_cannot_redirect_database_blobs_or_trace_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        store
            .db
            .execute_batch("PRAGMA journal_mode=DELETE")
            .unwrap();
        fs::rename(
            root.path().join(".greenlight/state"),
            root.path().join(".greenlight/original-state"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join(".greenlight/state")).unwrap();
        store.db.execute_batch("CREATE TABLE descriptor_probe(value); INSERT INTO descriptor_probe VALUES ('anchored')").unwrap();
        store
            .add_artifact(&session, "step", "report.md", "private artifact")
            .unwrap();
        let id = crate::conversation_dump::save_message(
            &store,
            &serde_json::json!({"evidence":"private trace"}),
        )
        .unwrap();
        assert_eq!(
            store.artifacts(&session, "step").unwrap()[0].content,
            "private artifact"
        );
        assert!(crate::conversation_dump::load_message(&store, &id).is_ok());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(Store::open(root.path()).is_err());
    }
    #[test]
    fn private_blob_and_database_links_do_not_follow_host_targets() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("private"), "host private").unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        store
            .add_artifact(&session, "step", "report.md", "old")
            .unwrap();
        let name: String = store
            .db
            .query_row("SELECT blob_path FROM artifacts LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        fs::remove_file(store.directory.join("blobs").join(&name)).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("private"),
            store.directory.join("blobs").join(&name),
        )
        .unwrap();
        assert!(store.artifacts(&session, "step").is_err());
        fs::rename(
            store.directory.join("blobs"),
            store.directory.join("old-blobs"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), store.directory.join("blobs")).unwrap();
        assert!(store
            .add_artifact(&session, "step", "next.md", "new")
            .is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
        let other = tempfile::tempdir().unwrap();
        fs::create_dir_all(other.path().join(".greenlight/state")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("private"),
            other.path().join(".greenlight/state/history.db"),
        )
        .unwrap();
        assert!(Store::open(other.path()).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("private")).unwrap(),
            "host private"
        );
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn history_cache_tracks_local_external_and_rolled_back_changes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let first = store.history(&session).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &first.0,
            &store.history(&session).unwrap().0
        ));
        let tx = store.db.unchecked_transaction().unwrap();
        store.event(&session, None, "test", "rolled back").unwrap();
        assert!(store
            .history(&session)
            .unwrap()
            .iter()
            .any(|e| e.3 == "rolled back"));
        tx.rollback().unwrap();
        assert!(!store
            .history(&session)
            .unwrap()
            .iter()
            .any(|e| e.3 == "rolled back"));
        let external = Store::open(root.path()).unwrap();
        external
            .event(&session, None, "test", "external commit")
            .unwrap();
        let refreshed = store.history(&session).unwrap();
        assert!(refreshed.iter().any(|e| e.3 == "external commit"));
        assert!(!std::sync::Arc::ptr_eq(&first.0, &refreshed.0));
        store.event(&session, None, "test", "local commit").unwrap();
        assert!(store
            .history(&session)
            .unwrap()
            .iter()
            .any(|e| e.3 == "local commit"));
        let mut edited = store.history(&session).unwrap();
        edited.clear();
        assert!(!store.history(&session).unwrap().is_empty());
    }
    #[test]
    fn history_refuses_large_bodies_but_preserves_previews_and_exact_small_records() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let small = store.event(&session, None, "test", "small").unwrap();
        let large = store
            .event(&session, None, "test", &"x".repeat(2048))
            .unwrap();
        store.limits.blob_bytes = 1024;
        assert!(store
            .history(&session)
            .unwrap_err()
            .downcast_ref::<crate::resource_limits::Exceeded>()
            .is_some());
        let previews = store.history_previews(&session).unwrap();
        assert!(previews
            .iter()
            .find(|e| e.0 == large)
            .unwrap()
            .3
            .contains("Preview only"));
        assert_eq!(store.event_body(&session, small).unwrap(), "small");
        assert!(store.event_body(&session, large).is_err());
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT length(data) FROM events WHERE id=?1",
                    [large],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap(),
            2048
        );
        store.limits.blob_bytes = 4096;
        store.limits.materialized_bytes = 1024;
        assert!(store.history(&session).is_err());
        store.limits.materialized_bytes = 8192;
        store.limits.history_events = 1;
        assert!(store.history(&session).is_err());
    }
    #[test]
    fn artifact_metadata_does_not_materialize_sparse_legacy_body_and_aggregate_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        store
            .add_artifact(&session, "step", "large.md", "small")
            .unwrap();
        let metadata = store.artifact_metadata(&session, "session").unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(store.directory.join("blobs").join(&metadata[0].blob))
            .unwrap();
        file.set_len(1 << 30).unwrap();
        assert_eq!(
            store.artifact_metadata(&session, "session").unwrap()[0].bytes,
            Some(1 << 30)
        );
        assert_eq!(store.blob_reads.get(), 0);
        assert!(store.artifacts(&session, "session").is_err());
        assert_eq!(file.metadata().unwrap().len(), 1 << 30);
        file.set_len(5).unwrap();
        store
            .add_artifact(&session, "step", "other.md", "12345")
            .unwrap();
        store.limits.materialized_bytes = 9;
        assert!(store.artifacts(&session, "session").is_err());
    }
    #[test]
    fn dump_refuses_count_aggregate_and_render_budgets_without_creating_export() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        store
            .event(&session, None, "test", &"x".repeat(500))
            .unwrap();
        store.limits.export_events = 1;
        assert!(crate::conversation_dump::Dump::load(&store, &session.id, None, false).is_err());
        store.limits.export_events = 100;
        store.limits.export_bytes = 100;
        assert!(crate::conversation_dump::Dump::load(&store, &session.id, None, false).is_err());
        store.limits.export_bytes = 1200;
        let dump = crate::conversation_dump::Dump::load(&store, &session.id, None, false).unwrap();
        let path = root.path().join("dump.md");
        assert!(dump.save(&path, false).is_err());
        assert!(!path.exists());
    }
}
