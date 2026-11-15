//! Read-only session discovery and validated creation for the TUI.
use crate::{
    config,
    store::{Session, Store},
};
use anyhow::{bail, Result};
use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug)]
pub struct WorkflowChoice {
    pub id: String,
    pub title: String,
    pub error: Option<String>,
}
#[derive(Default)]
pub struct NewSession {
    pub workflows: Vec<WorkflowChoice>,
    pub cursor: usize,
    pub goal: Option<String>,
    pub error: Option<String>,
}
impl NewSession {
    pub fn discover(root: &Path) -> Result<Self> {
        let directory = root.join("workflows");
        let mut workflows = Vec::new();
        match std::fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if !entry.file_type()?.is_dir() {
                        continue;
                    }
                    let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let (title, error) = match config::load_workflow(root, &id) {
                        Ok(wf) => (wf.definition.title, None),
                        Err(error) => (id.clone(), Some(format!("{error:#}"))),
                    };
                    workflows.push(WorkflowChoice { id, title, error });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        workflows.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self {
            workflows,
            ..Self::default()
        })
    }
    pub fn create(&self, root: &Path, store: &Store) -> Result<Session> {
        let Some(choice) = self.workflows.get(self.cursor) else {
            bail!("No workflow selected")
        };
        let goal = self.goal.as_deref().unwrap_or("").trim();
        if goal.is_empty() {
            bail!("Enter a goal before creating a session")
        }
        // Reload at submission: workflow files may have changed while the form was open.
        let wf = config::load_workflow(root, &choice.id)?;
        store.ensure_no_pending_writes()?;
        store.create(&choice.id, goal, &wf.definition.start)
    }
}

pub type Activity = HashMap<String, (i64, String)>;
impl Store {
    pub fn session_activity(&self) -> Result<Activity> {
        let mut query=self.db.prepare("SELECT session,id,timestamp FROM events WHERE id IN (SELECT MAX(id) FROM events GROUP BY session)")?;
        let rows = query.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?;
        Ok(rows.collect::<rusqlite::Result<Activity>>()?)
    }
}
pub fn filter_and_sort(sessions: &mut Vec<Session>, query: &str, activity: &Activity) {
    let query = query.to_lowercase();
    sessions.retain(|s| {
        [&s.goal, &s.workflow, &s.current_step, &s.status, &s.id]
            .iter()
            .any(|text| text.to_lowercase().contains(&query))
    });
    sessions.sort_by(|a, b| {
        (b.status == "blocked")
            .cmp(&(a.status == "blocked"))
            .then_with(|| {
                activity
                    .get(&b.id)
                    .map(|v| v.0)
                    .cmp(&activity.get(&a.id).map(|v| v.0))
            })
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workflow(root: &Path) {
        let folder = root.join("workflows/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo workflow\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("work.md"),
            "---\nid: work\nverification:\n  type: none\n---\nWork\n",
        )
        .unwrap();
    }
    #[test]
    fn blocked_first_then_activity_and_case_insensitive_search() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut blocked = store.create("demo", "Écrire parser", "work").unwrap();
        blocked.status = "blocked".into();
        store.save(&blocked).unwrap();
        let newer = store.create("demo", "Recent goal", "review").unwrap();
        let older = store.create("demo", "Another goal", "work").unwrap();
        store.event(&newer, None, "user.message", "latest").unwrap();
        let activity = store.session_activity().unwrap();
        let mut sessions = store.list().unwrap();
        filter_and_sort(&mut sessions, "", &activity);
        assert_eq!(
            sessions.iter().map(|s| &s.id).collect::<Vec<_>>(),
            [&blocked.id, &newer.id, &older.id]
        );
        for query in ["ÉCRIRE", "BLOCKED", &blocked.id] {
            let mut sessions = store.list().unwrap();
            filter_and_sort(&mut sessions, query, &activity);
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].id, blocked.id);
        }
        filter_and_sort(&mut sessions, "no matches", &activity);
        assert!(sessions.is_empty());
    }
    #[test]
    fn discovery_exposes_invalid_workflows_and_creation_revalidates_files() {
        let root = tempfile::tempdir().unwrap();
        workflow(root.path());
        std::fs::create_dir_all(root.path().join("workflows/broken")).unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut form = NewSession::discover(root.path()).unwrap();
        assert_eq!(form.workflows.len(), 2);
        assert!(form.workflows[0].error.is_some());
        form.cursor = 1;
        form.goal = Some("   ".into());
        assert!(form.create(root.path(), &store).is_err());
        form.goal = Some("  A clear goal  ".into());
        std::fs::remove_file(root.path().join("workflows/demo/work.md")).unwrap();
        assert!(form.create(root.path(), &store).is_err());
        assert!(store.list().unwrap().is_empty());
        workflow(root.path());
        let session = form.create(root.path(), &store).unwrap();
        assert_eq!(session.goal, "A clear goal");
        assert_eq!(session.status, "ready");
        assert_eq!(store.history(&session).unwrap().len(), 1);
        assert!(store.attempts(&session).unwrap().is_empty());
    }
    #[test]
    fn empty_project_has_no_workflows_and_no_side_effects() {
        let root = tempfile::tempdir().unwrap();
        assert!(NewSession::discover(root.path())
            .unwrap()
            .workflows
            .is_empty());
        assert!(!root.path().join("workflows").exists());
    }
}
