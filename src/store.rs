use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};
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
pub struct Artifact { pub name: String, pub content: String, pub step: String }

pub struct Store { db: Connection, pub directory: PathBuf }
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let directory = root.join(".stepwright").join("state");
        fs::create_dir_all(&directory)?;
        let db = Connection::open(directory.join("history.db"))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
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
        self.db.execute("INSERT INTO events(session,branch,step,kind,data) VALUES (?1,?2,?3,?4,?5)", params![s.id,s.active_branch,step,kind,data])?;
        Ok(self.db.last_insert_rowid())
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
        if path.is_absolute() || path.components().any(|p| !matches!(p,std::path::Component::Normal(_))) { bail!("artifact path must be relative, without .."); }
        let ev = self.event(s,Some(step),"artifact.created",name)?;
        let blob = format!("{}-{}.txt",ev,Uuid::new_v4());
        let dir = self.directory.join("blobs");
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(&blob),content)?;
        self.db.execute("INSERT INTO artifacts(session,branch,event_id,step,name,blob_path) VALUES (?1,?2,?3,?4,?5,?6)", params![s.id,s.active_branch,ev,step,name,blob])?;
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
    pub fn current_run_artifacts(&self,s:&Session,step:&str)->Result<Vec<String>> {
        let marker:i64=self.db.query_row("SELECT id FROM events WHERE session=?1 AND branch=?2 AND step=?3 AND kind='step.started' ORDER BY id DESC LIMIT 1",
            params![s.id,s.active_branch,step],|r|r.get(0)).optional()?.unwrap_or(i64::MAX);
        let mut st=self.db.prepare("SELECT DISTINCT name FROM artifacts WHERE session=?1 AND branch=?2 AND step=?3 AND event_id>?4")?;
        let rows=st.query_map(params![s.id,s.active_branch,step,marker],|r|r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn artifacts(&self,s:&Session, source: &str) -> Result<Vec<Artifact>> {
        let mut versions: std::collections::BTreeMap<String,Artifact> = std::collections::BTreeMap::new();
        for (branch,cutoff) in self.lineage(&s.active_branch)? {
            let mut st = self.db.prepare("SELECT step,name,blob_path FROM artifacts WHERE session=?1 AND branch=?2 AND event_id<=?3 ORDER BY event_id")?;
            let rows = st.query_map(params![s.id,branch,cutoff],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?;
            for row in rows {
                let (step,name,blob) = row?;
                if source != "session" && source != step { continue; }
                let content = fs::read_to_string(self.directory.join("blobs").join(blob))?;
                versions.insert(name.clone(), Artifact{name,content,step});
            }
        }
        Ok(versions.into_values().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
