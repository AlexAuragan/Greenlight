use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::{Path, PathBuf}};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String, pub workflow: String, pub goal: String,
    pub active_branch: String, pub current_step: String, pub status: String,
}
#[derive(Debug, Clone)]
pub struct Branch { pub id: String, pub parent: Option<String>, pub fork_event: Option<i64> }
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
pub struct Artifact { pub name: String, pub content: String, pub step: String, pub event_id: i64 }

pub struct Store { pub(crate) db: Connection, pub directory: PathBuf }
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let directory = crate::paths::state_directory(root)?;
        let db = Connection::open(directory.join("history.db"))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
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
           CREATE TABLE IF NOT EXISTS step_attempts (
             id TEXT PRIMARY KEY, session TEXT NOT NULL, branch TEXT NOT NULL,
             step TEXT NOT NULL, definition_event INTEGER NOT NULL, start_event INTEGER NOT NULL,
             end_event INTEGER, status TEXT NOT NULL);
           CREATE INDEX IF NOT EXISTS restores_branch ON restore_changes(session,branch,event_id);
           CREATE INDEX IF NOT EXISTS changes_branch ON file_changes(session,branch,event_id);
           CREATE INDEX IF NOT EXISTS event_branch ON events(session,branch,id);
           CREATE INDEX IF NOT EXISTS artifact_branch ON artifacts(session,branch,event_id);")?;
        Ok(Self { db, directory })
    }
    pub fn create(&self, workflow: &str, goal: &str, start: &str) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let branch = Uuid::new_v4().to_string();
        let session = Session { id: id.clone(), workflow: workflow.into(), goal: goal.into(), active_branch: branch.clone(), current_step: start.into(), status: "ready".into() };
        self.db.execute("INSERT INTO sessions VALUES (?1,?2,?3,?4,?5,?6)", params![id, workflow, goal, branch, start, "ready"])?;
        self.db.execute("INSERT INTO branches(id,session,parent,fork_event) VALUES (?1,?2,NULL,NULL)", params![branch,id])?;
        self.event(&session, Some(start), "session.started", goal)?;
        Ok(session)
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        self.db.query_row("SELECT id, workflow, goal, active_branch, current_step, status FROM sessions WHERE id=?1", [id], |r| Ok(Session { id:r.get(0)?, workflow:r.get(1)?, goal:r.get(2)?, active_branch:r.get(3)?, current_step:r.get(4)?, status:r.get(5)? })).map_err(Into::into)
    }
    pub fn list(&self) -> Result<Vec<Session>> {
        let mut st = self.db.prepare("SELECT id, workflow, goal, active_branch, current_step, status FROM sessions ORDER BY rowid DESC")?;
        let rows = st.query_map([], |r| Ok(Session { id:r.get(0)?, workflow:r.get(1)?, goal:r.get(2)?, active_branch:r.get(3)?, current_step:r.get(4)?, status:r.get(5)? }))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn save(&self, s: &Session) -> Result<()> {
        self.db.execute("UPDATE sessions SET active_branch=?2,current_step=?3,status=?4 WHERE id=?1", params![s.id,s.active_branch,s.current_step,s.status])?;
        Ok(())
    }
    pub fn event(&self, s: &Session, step: Option<&str>, kind: &str, data: &str) -> Result<i64> {
        let tx = if self.db.is_autocommit() { Some(self.db.unchecked_transaction()?) } else { None };
        self.db.execute("INSERT INTO events(session,branch,step,kind,data) VALUES (?1,?2,?3,?4,?5)", params![s.id,s.active_branch,step,kind,data])?;
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
        if let Some(tx) = tx { tx.commit()?; }
        Ok(event)
    }
    pub fn branch(&self, id: &str) -> Result<Branch> {
        self.db.query_row("SELECT id,parent,fork_event FROM branches WHERE id=?1", [id], |r| Ok(Branch{id:r.get(0)?,parent:r.get(1)?,fork_event:r.get(2)?})).map_err(Into::into)
    }
    pub fn branches(&self, session:&str) -> Result<Vec<Branch>> {
        let mut st=self.db.prepare("SELECT id,parent,fork_event FROM branches WHERE session=?1 ORDER BY rowid")?;
        let rows=st.query_map([session],|r|Ok(Branch{id:r.get(0)?,parent:r.get(1)?,fork_event:r.get(2)?}))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn switch_branch(&self,s:&mut Session,branch:&str,step:&str)->Result<()> {
        let _lock = self.lock_idle_session(&s.id)?;
        self.ensure_no_pending_writes()?;
        let found:i64=self.db.query_row("SELECT count(*) FROM branches WHERE session=?1 AND id=?2",params![s.id,branch],|r|r.get(0))?;
        if found!=1 {bail!("branch does not belong to this session");}
        s.active_branch=branch.into();
        s.current_step=step.into();
        s.status="ready".into();
        self.save(s)?;
        self.event(s,Some(step),"branch.selected","manual selection; filesystem unchanged")?;
        Ok(())
    }
    pub fn fork(&self, s: &mut Session, event_id: i64, step: &str) -> Result<()> {
        let _lock = self.lock_idle_session(&s.id)?;
        self.ensure_no_pending_writes()?;
        let valid: bool = self.history(s)?.iter().any(|(id,_,_,_)| *id == event_id);
        if !valid { bail!("fork event is not part of current branch ancestry"); }
        let branch = Uuid::new_v4().to_string();
        self.db.execute("INSERT INTO branches(id,session,parent,fork_event) VALUES (?1,?2,?3,?4)", params![branch,s.id,s.active_branch,event_id])?;
        s.active_branch = branch;
        s.current_step = step.into();
        s.status = "ready".into();
        self.save(s)?;
        self.event(s,Some(step),"branch.forked",&format!("from_event: {event_id}"))?;
        Ok(())
    }
    fn lineage(&self, branch: &str) -> Result<Vec<(String,i64)>> {
        let mut line = vec![];
        let mut cursor = branch.to_string();
        let mut cutoff = i64::MAX;
        loop {
            line.push((cursor.clone(),cutoff));
            let b = self.branch(&cursor)?;
            match (b.parent,b.fork_event) {
                (Some(parent),Some(fork)) => { cursor=parent; cutoff=cutoff.min(fork); },
                (None,None) => break,
                _ => bail!("corrupt branch ancestry"),
            }
        }
        line.reverse();
        Ok(line)
    }
    pub fn history(&self, s: &Session) -> Result<Vec<(i64,String,String,String)>> {
        let mut result = vec![];
        for (branch,cutoff) in self.lineage(&s.active_branch)? {
            let mut st = self.db.prepare("SELECT id,coalesce(step,''),kind,data FROM events WHERE session=?1 AND branch=?2 AND id<=?3 ORDER BY id")?;
            let rows = st.query_map(params![s.id,branch,cutoff],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            result.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        Ok(result)
    }
    pub fn add_artifact(&self,s:&Session,step:&str,name:&str,content:&str) -> Result<()> {
        let path = Path::new(name);
        if name.is_empty() || path.is_absolute() || path.components().any(|p| !matches!(p,std::path::Component::Normal(_))) { bail!("artifact path must be relative, without .."); }
        let blob = format!("artifact-{}.txt", Uuid::new_v4());
        let dir = self.directory.join("blobs");
        fs::create_dir_all(&dir)?;
        fs::File::open(&self.directory)?.sync_all()?;
        let blob_path = dir.join(&blob);
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&blob_path)?;
        let publication: Result<()> = (|| {
            // Make the immutable body durable before publishing references.
            // A crash here may leave an unreferenced blob, never a success event.
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            fs::File::open(&dir)?.sync_all()?;
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
            if let Err(cleanup) = fs::remove_file(&blob_path) {
                return Err(error.context(format!("artifact publication failed; could not remove unpublished blob {}: {cleanup}", blob_path.display())));
            }
            return Err(error.context("artifact publication failed"));
        }
        Ok(())
    }
    pub fn add_change(&self, s: &Session, step: &str, path: &str, before: Option<&str>, after: &str) -> Result<i64> {
        let dir = self.directory.join("blobs");
        fs::create_dir_all(&dir)?;
        let before_name = before.map(|_| format!("edit-before-{}.txt", Uuid::new_v4()));
        let after_name = format!("edit-after-{}.txt", Uuid::new_v4());
        if let (Some(content), Some(name)) = (before, &before_name) {
            fs::write(dir.join(name), content)?;
        }
        fs::write(dir.join(&after_name), after)?;
        // Recovered native writes publish references only after their snapshots
        // are durable. The intent's SQLite transaction owns publication.
        if let Some(name) = &before_name { fs::File::open(dir.join(name))?.sync_all()?; }
        fs::File::open(dir.join(&after_name))?.sync_all()?;
        fs::File::open(&dir)?.sync_all()?;
        fs::File::open(&self.directory)?.sync_all()?;
        let event_id = self.event(s, Some(step), "file.changed", path)?;
        self.db.execute(
            "INSERT INTO file_changes(event_id,session,branch,step,path,before_blob,after_blob) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![event_id, s.id, s.active_branch, step, path, before_name, after_name],
        )?;
        Ok(event_id)
    }

    pub fn changes(&self, s: &Session) -> Result<Vec<FileChange>> {
        let mut result = Vec::new();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut stmt = self.db.prepare(
                "SELECT event_id,step,path,before_blob,after_blob FROM file_changes WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id"
            )?;
            let rows = stmt.query_map(params![s.id,branch,cutoff], |r| Ok((
                r.get::<_,i64>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?,
                r.get::<_,Option<String>>(3)?, r.get::<_,String>(4)?
            )))?;
            for row in rows {
                let (event_id,step,path,before_blob,after_blob) = row?;
                let before = before_blob.map(|b| fs::read_to_string(self.directory.join("blobs").join(b))).transpose()?;
                let after = fs::read_to_string(self.directory.join("blobs").join(after_blob))?;
                result.push(FileChange { event_id,step,path,before,after });
            }
        }
        Ok(result)
    }
    /// All native writes and explicit restores visible on this branch, in event order.
    /// Restores are kept separate because they may remove a file entirely.
    pub fn workspace_edits(&self, s: &Session) -> Result<Vec<WorkspaceEdit>> {
        let mut edits: Vec<WorkspaceEdit> = self.changes(s)?
            .into_iter()
            .map(|change| WorkspaceEdit {
                event_id: change.event_id,
                path: change.path,
                before: change.before,
                after: Some(change.after),
            })
            .collect();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut stmt = self.db.prepare(
                "SELECT event_id,path,before_blob,after_blob
                 FROM restore_changes WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id"
            )?;
            let rows = stmt.query_map(params![s.id, branch, cutoff], |r| Ok((
                r.get::<_,i64>(0)?,
                r.get::<_,String>(1)?,
                r.get::<_,Option<String>>(2)?,
                r.get::<_,Option<String>>(3)?,
            )))?;
            for row in rows {
                let (event_id, path, before_blob, after_blob) = row?;
                let before = before_blob
                    .map(|blob| fs::read_to_string(self.directory.join("blobs").join(blob)))
                    .transpose()?;
                let after = after_blob
                    .map(|blob| fs::read_to_string(self.directory.join("blobs").join(blob)))
                    .transpose()?;
                edits.push(WorkspaceEdit { event_id, path, before, after });
            }
        }
        edits.sort_by_key(|edit| edit.event_id);
        Ok(edits)
    }

    /// Journal one already-applied workspace restore, including file deletions.
    pub fn add_restoration(
        &self, s: &Session, path: &str, before: Option<&str>, after: Option<&str>,
    ) -> Result<i64> {
        let dir = self.directory.join("blobs");
        fs::create_dir_all(&dir)?;
        let save = |content: Option<&str>| -> Result<Option<String>> {
            match content {
                Some(text) => {
                    let name = format!("restore-{}.txt", Uuid::new_v4());
                    fs::write(dir.join(&name), text)?;
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
            params![event_id, s.id, s.active_branch, path, before_blob, after_blob],
        )?;
        Ok(event_id)
    }

    pub fn current_run_artifacts(&self,s:&Session,step:&str)->Result<Vec<String>> {
        Ok(self.attempt_artifacts(s, step)?.into_iter().map(|a| a.name).collect())
    }
    pub fn begin_attempt(&self, s: &Session, step: &str, definition: &str, model: &str) -> Result<String> {
        let tx = self.db.unchecked_transaction()?;
        let definition_event = self.event(s, Some(step), "step.definition", definition)?;
        let id = Uuid::new_v4().to_string();
        let start = self.event(s, Some(step), "step.started", &format!("model: {model}\nattempt: {id}"))?;
        self.db.execute("UPDATE step_attempts SET status='interrupted',end_event=?2 WHERE session=?1 AND status='running'", params![s.id,start-1])?;
        self.db.execute("INSERT INTO step_attempts(id,session,branch,step,definition_event,start_event,status) VALUES (?1,?2,?3,?4,?5,?6,'running')",
            params![id,s.id,s.active_branch,step,definition_event,start])?;
        tx.commit()?;
        Ok(id)
    }
    pub fn active_attempt(&self, s: &Session, step: &str) -> Result<(String,i64)> {
        self.db.query_row("SELECT id,start_event FROM step_attempts WHERE session=?1 AND branch=?2 AND step=?3 AND status='running' ORDER BY start_event DESC LIMIT 1",
            params![s.id,s.active_branch,step], |r| Ok((r.get(0)?,r.get(1)?))).map_err(Into::into)
    }
    pub fn attempt_artifacts(&self, s: &Session, step: &str) -> Result<Vec<Artifact>> {
        let (_, start) = self.active_attempt(s, step)?;
        let mut query = self.db.prepare("SELECT name,blob_path,event_id FROM artifacts WHERE session=?1 AND branch=?2 AND step=?3 AND event_id>?4 ORDER BY event_id")?;
        let rows = query.query_map(params![s.id,s.active_branch,step,start], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?;
        let mut versions = std::collections::BTreeMap::new();
        for row in rows {
            let (name,blob,event_id) = row?;
            versions.insert(name.clone(), Artifact {name,content:fs::read_to_string(self.directory.join("blobs").join(blob))?,step:step.into(),event_id});
        }
        Ok(versions.into_values().collect())
    }
    pub fn attempt_history(&self, s: &Session) -> Result<Vec<String>> {
        let mut result = Vec::new();
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let mut query = self.db.prepare("SELECT id,step,start_event,end_event,status FROM step_attempts WHERE session=?1 AND branch=?2 AND start_event<=?3 ORDER BY start_event")?;
            let rows = query.query_map(params![s.id,branch,cutoff], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?,r.get::<_,String>(4)?)))?;
            for row in rows {
                let (id,step,start,end,status) = row?;
                let status = if end.is_none_or(|end| end > cutoff) && cutoff != i64::MAX { "inherited-partial" } else { &status };
                result.push(format!("{id}  {step}  {status}  events {start}..{}",end.filter(|end| *end <= cutoff).map(|end| end.to_string()).unwrap_or_else(|| "open".into())));
            }
        }
        Ok(result)
    }
    pub fn artifacts(&self,s:&Session, source: &str) -> Result<Vec<Artifact>> {
        let mut versions: std::collections::BTreeMap<String,Artifact> = std::collections::BTreeMap::new();
        for (branch,cutoff) in self.lineage(&s.active_branch)? {
            let mut st = self.db.prepare("SELECT step,name,blob_path,event_id FROM artifacts WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id")?;
            let rows = st.query_map(params![s.id,branch,cutoff],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?)))?;
            for row in rows {
                let (step,name,blob,event_id) = row?;
                if source != "session" && source != step { continue; }
                let content = fs::read_to_string(self.directory.join("blobs").join(blob))?;
                versions.insert(name.clone(), Artifact{name,content,step,event_id});
            }
        }
        Ok(versions.into_values().collect())
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
        let first = store.begin_attempt(&session, "step", "first definition", "fixture").unwrap();
        store.add_artifact(&session, "step", "report.md", "old success").unwrap();
        store.event(&session, Some("step"), "step.blocked", "retry").unwrap();
        let second = store.begin_attempt(&session, "step", "second definition", "fixture").unwrap();
        assert_ne!(first,second);
        assert!(store.current_run_artifacts(&session,"step").unwrap().is_empty());
        store.add_artifact(&session,"step","report.md","current failure").unwrap();
        let version = store.attempt_artifacts(&session,"step").unwrap().remove(0);
        assert_eq!(version.content,"current failure");
        assert!(version.event_id > store.active_attempt(&session,"step").unwrap().1);
        drop(store);
        let reopened = Store::open(root.path()).unwrap();
        assert_eq!(reopened.active_attempt(&session,"step").unwrap().0,second);
        let attempts = reopened.attempt_history(&session).unwrap();
        assert_eq!(attempts.len(),2);
        assert!(attempts[0].contains(&first) && attempts[0].contains("blocked"));
        assert!(attempts[1].contains(&second) && attempts[1].contains("running"));
    }

    #[test]
    fn forked_attempt_history_honors_cutoffs_and_recovery_closes_interrupted_attempts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("demo","goal","step").unwrap();
        let owner = store.claim_execution(&session.id,None).unwrap();
        let parent = store.begin_attempt(&session,"step","definition","fixture").unwrap();
        let cutoff = store.active_attempt(&session,"step").unwrap().1;
        drop(owner);
        store.recover_session(&session.id).unwrap();
        assert!(store.attempt_history(&session).unwrap()[0].contains("interrupted"));
        store.fork(&mut session,cutoff,"step").unwrap();
        let child = store.begin_attempt(&session,"step","child definition","fixture").unwrap();
        let history = store.attempt_history(&session).unwrap();
        assert_eq!(history.len(),2);
        assert!(history[0].contains(&parent) && history[0].contains("inherited-partial"));
        assert!(history[1].contains(&child));
        assert!(store.current_run_artifacts(&session,"step").unwrap().is_empty());
    }

    #[test]
    fn attempt_creation_failure_rolls_back_its_definition_and_start() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo","goal","step").unwrap();
        let before = store.history(&session).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_attempt BEFORE INSERT ON step_attempts BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        assert!(store.begin_attempt(&session,"step","definition","fixture").is_err());
        assert_eq!(store.history(&session).unwrap(),before);
    }
    #[test]
    fn artifact_blob_failure_does_not_publish_a_success_event() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "plan").unwrap();
        let before = store.history(&session).unwrap();
        // A file in place of the blob directory deterministically fails I/O.
        fs::write(store.directory.join("blobs"), "obstruction").unwrap();
        assert!(store.add_artifact(&session, "plan", "plan.md", "draft").is_err());
        assert_eq!(store.history(&session).unwrap(), before);
        assert!(store.artifacts(&session, "session").unwrap().is_empty());
        fs::remove_file(store.directory.join("blobs")).unwrap();
        store.add_artifact(&session, "plan", "plan.md", "retry").unwrap();
        assert_eq!(store.artifacts(&session, "session").unwrap()[0].content, "retry");
    }

    #[test]
    fn artifact_database_failure_rolls_back_publication_and_preserves_previous_version() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "plan").unwrap();
        store.add_artifact(&session, "plan", "plan.md", "original").unwrap();
        let before = store.history(&session).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_artifact BEFORE INSERT ON artifacts
            BEGIN SELECT RAISE(ABORT, 'injected artifact insert failure'); END;").unwrap();
        assert!(store.add_artifact(&session, "plan", "plan.md", "replacement").is_err());
        assert_eq!(store.history(&session).unwrap(), before);
        assert_eq!(store.artifacts(&session, "session").unwrap()[0].content, "original");
        assert_eq!(fs::read_dir(store.directory.join("blobs")).unwrap().count(), 1);
        store.db.execute_batch("DROP TRIGGER reject_artifact;").unwrap();
        store.add_artifact(&session, "plan", "plan.md", "retry").unwrap();
        drop(store);
        let reopened = Store::open(root.path()).unwrap();
        assert_eq!(reopened.artifacts(&session, "session").unwrap()[0].content, "retry");
        assert_eq!(reopened.history(&session).unwrap().iter()
            .filter(|(_, _, kind, _)| kind == "artifact.created").count(), 2);
    }

    #[test]
    fn artifact_commit_failure_rolls_back_event_and_blob() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "plan").unwrap();
        let before = store.history(&session).unwrap();
        // Defer a foreign-key violation so both INSERTs succeed and COMMIT fails.
        store.db.execute_batch("PRAGMA defer_foreign_keys=ON;
            CREATE TRIGGER fail_commit BEFORE INSERT ON artifacts BEGIN
              INSERT INTO branches(id,session) VALUES ('injected-branch','missing-session');
            END;").unwrap();
        let error = store.add_artifact(&session, "plan", "plan.md", "draft").unwrap_err();
        assert!(format!("{error:#}").contains("FOREIGN KEY constraint failed"));
        assert_eq!(store.history(&session).unwrap(), before);
        assert!(store.artifacts(&session, "session").unwrap().is_empty());
        assert_eq!(fs::read_dir(store.directory.join("blobs")).unwrap().count(), 0);
        store.db.execute_batch("DROP TRIGGER fail_commit;").unwrap();
        store.add_artifact(&session, "plan", "plan.md", "retry").unwrap();
        assert_eq!(store.artifacts(&session, "session").unwrap()[0].content, "retry");
    }

    #[test]
    fn old_state_migrates_with_session_and_artifacts_intact() {
        let root = tempfile::tempdir().unwrap();
        let session_id;
        {
            let store = Store::open(root.path()).unwrap();
            let session = store.create("demo", "original goal", "plan").unwrap();
            store.add_artifact(&session, "plan", "plan.md", "original artifact").unwrap();
            session_id = session.id;
        }
        let old_dir = root.path().join(".stepwright");
        fs::create_dir(&old_dir).unwrap();
        fs::rename(root.path().join(".greenlight/state"), old_dir.join("state")).unwrap();

        let reopened = Store::open(root.path()).unwrap();
        let session = reopened.session(&session_id).unwrap();
        assert_eq!(session.goal, "original goal");
        assert_eq!(reopened.artifacts(&session, "session").unwrap()[0].content, "original artifact");
        assert!(!old_dir.join("state").exists());
        assert!(root.path().join(".greenlight/state/history.db").is_file());
    }

    #[test]
    fn fork_preserves_history_and_artifacts() {
        let d=tempfile::tempdir().unwrap(); let store=Store::open(d.path()).unwrap();
        let mut s=store.create("test","goal","a").unwrap();
        store.add_artifact(&s,"a","plan.md","old").unwrap();
        let mark=store.history(&s).unwrap().last().unwrap().0;
        store.add_artifact(&s,"a","plan.md","should disappear").unwrap();
        store.fork(&mut s,mark,"a").unwrap();
        assert_eq!(store.artifacts(&s,"session").unwrap()[0].content,"old");
        assert!(!store.history(&s).unwrap().iter().any(|(_,_,_,d)|d=="should disappear"));
    }
}
