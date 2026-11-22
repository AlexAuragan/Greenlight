//! OS locks establish live ownership; persistent records identify interrupted runs.
use crate::store::Store;
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs;

pub struct ExecutionOwner {
    _lock: crate::locks::LockGuard,
    project: std::sync::Arc<crate::project_execution::ProjectLease>,
    token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionState {
    Idle,
    Live,
    Interrupted,
}

impl Store {
    pub fn execution_state(&self, id: &str) -> Result<ExecutionState> {
        // Do not briefly lock idle sessions during polling: a resume may start concurrently.
        if !self.has_execution_record(id)?
            && self.session(id)?.status != "running"
            && self.project_record()?.is_none_or(|r| r.session != id)
        {
            return Ok(ExecutionState::Idle);
        }
        match self.session_lock(id) {
            Ok(_lock) => Ok(
                if self.has_execution_record(id)?
                    || self.session(id)?.status == "running"
                    || self.project_record()?.is_some_and(|r| r.session == id)
                {
                    ExecutionState::Interrupted
                } else {
                    ExecutionState::Idle
                },
            ),
            Err(error)
                if error
                    .downcast_ref::<std::fs::TryLockError>()
                    .is_some_and(|e| matches!(e, std::fs::TryLockError::WouldBlock)) =>
            {
                Ok(ExecutionState::Live)
            }
            Err(error) => Err(error),
        }
    }
    pub(crate) fn session_lock(&self, id: &str) -> Result<crate::locks::LockGuard> {
        self.session(id)?;
        let directory = self
            .directory_fd
            .create_dirs(std::path::Path::new("session-locks"))?;
        let name = format!("{:x}.lock", Sha256::digest(id.as_bytes()));
        let lock = directory.lock_file(std::path::Path::new(&name))?;
        let lock = crate::locks::LockGuard::acquire(lock)
            .context("session already has a live execution owner or control operation")?;
        Ok(lock)
    }

    fn has_execution_record(&self, id: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM execution_owners WHERE session=?1",
                [id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn lock_idle_session(&self, id: &str) -> Result<crate::locks::LockGuard> {
        let lock = self.session_lock(id)?;
        if self.has_execution_record(id)? || self.session(id)?.status == "running" {
            bail!("session has an interrupted execution; run greenlight recover-session {id} (or /recover in the TUI)");
        }
        Ok(lock)
    }

    pub fn claim_execution(&self, id: &str, instruction: Option<&str>) -> Result<ExecutionOwner> {
        self.claim_owner(id, instruction, None, None)
    }

    pub fn claim_tool_execution(&self, id: &str, branch: &str) -> Result<ExecutionOwner> {
        self.claim_owner(id, None, Some(branch), None)
    }

    pub(crate) fn claim_routed_execution(&self, id: &str, message: &str) -> Result<ExecutionOwner> {
        self.claim_owner(id, None, None, Some(message))
    }
    fn claim_owner(
        &self,
        id: &str,
        instruction: Option<&str>,
        rerun_branch: Option<&str>,
        route: Option<&str>,
    ) -> Result<ExecutionOwner> {
        let initial = self.session(id)?;
        let project = self.acquire_project(
            &initial,
            if rerun_branch.is_some() {
                "tool-rerun"
            } else {
                "workflow"
            },
        )?;
        let lock = self.lock_idle_session(id)?;
        self.ensure_no_pending_writes()?;
        let mut session = self.session(id)?;
        if initial.active_branch != session.active_branch {
            bail!("Session branch changed before execution claim");
        }
        if rerun_branch.is_some_and(|branch| branch != session.active_branch) {
            bail!("Branch changed; select the tool call again");
        }
        if !matches!(session.status.as_str(), "ready" | "blocked")
            && !(rerun_branch.is_some() && session.status == "completed")
        {
            bail!(
                "cannot run session in {} state; completed sessions require a fork",
                session.status
            );
        }
        if let Some(message) = route {
            let valid = self.manager_routes(&session)?.iter().any(|r| {
                r.message_id == message
                    && r.step == session.current_step
                    && r.state == "queued"
                    && r.attempt.is_none()
            });
            if !valid {
                bail!("Routing target/branch changed or instruction was already consumed; no new attempt started");
            }
        }
        let token = project.record.token.clone();
        let tx = self.db.unchecked_transaction()?;
        self.publish_project(&project)?;
        tx.execute(
            "INSERT INTO execution_owners(session,token,pid) VALUES (?1,?2,?3)",
            params![id, token, std::process::id()],
        )?;
        session.status = "running".into();
        self.save(&session)?;
        self.event(
            &session,
            Some(&session.current_step),
            "runner.started",
            &token,
        )?;
        if let Some(text) = instruction.filter(|text| !text.trim().is_empty()) {
            self.event(
                &session,
                Some(&session.current_step),
                "user.instruction",
                text,
            )?;
        }
        tx.commit()?;
        *self.active_project.borrow_mut() = std::sync::Arc::downgrade(&project);
        Ok(ExecutionOwner {
            _lock: lock,
            project,
            token,
        })
    }

    pub fn finish_execution(
        &self,
        id: &str,
        owner: &ExecutionOwner,
        error: Option<&anyhow::Error>,
    ) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let scope = self.session(id)?;
        self.invalidate_manager_routes(&scope,scope.status!="completed","Worker ended before request inclusion; review rather than replay against another attempt")?;
        self.release_project(&owner.project)?;
        let removed = tx.execute(
            "DELETE FROM execution_owners WHERE session=?1 AND token=?2",
            params![id, owner.token],
        )?;
        if removed != 1 {
            bail!("execution ownership changed unexpectedly");
        }
        let mut session = self.session(id)?;
        if session.status == "running" {
            session.status = "blocked".into();
            self.save(&session)?;
        }
        if let Some(error) = error {
            self.event(
                &session,
                Some(&session.current_step),
                "runner.error",
                &format!("{error:#}"),
            )?;
        }
        self.event(
            &session,
            Some(&session.current_step),
            "runner.stopped",
            &owner.token,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn recover_session(&self, id: &str) -> Result<String> {
        // Never infer death from PID reuse, elapsed time, or a UI's status.
        let _project = self.project_lock()?;
        let project = self.project_record()?;
        if project.as_ref().is_some_and(|r| r.session != id) {
            bail!(
                "Another interrupted project owner must be recovered first: {}",
                project.as_ref().unwrap().session
            );
        }
        let _lock = self.session_lock(id)?;
        let mut session = self.session(id)?;
        if project.is_none() && !self.has_execution_record(id)? && session.status != "running" {
            return Ok("No interrupted execution to recover; session unchanged.".into());
        }
        let tx = self.db.unchecked_transaction()?;
        if let Some(project) = &project {
            let session_token: Option<String> = self
                .db
                .query_row(
                    "SELECT token FROM execution_owners WHERE session=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            if session_token.as_ref().is_some_and(|t| t != &project.token) {
                bail!("Inconsistent project/session owner tokens; inspect ownership records before recovery");
            }
            tx.execute(
                "DELETE FROM project_owner WHERE singleton=1 AND token=?1",
                [&project.token],
            )?;
        }
        self.invalidate_manager_routes(
            &session,
            true,
            "Owner recovery cannot replay undelivered attempt-bound instructions",
        )?;
        tx.execute("DELETE FROM execution_owners WHERE session=?1", [id])?;
        if session.status != "completed" {
            session.status = "blocked".into();
        }
        self.save(&session)?;
        self.event(&session, Some(&session.current_step), "runner.recovered",
            "Owner lock was available. Previous worker's unanswered approvals expire; saved grants are unchanged. No work was resumed.")?;
        tx.commit()?;
        Ok(format!("Recovered session {id}; status: {}. Previous unanswered approvals expired; saved grants are unchanged. Inspect history and recover any interrupted writes before further work. Recovery did not resume execution.", session.status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_owner_excludes_competing_execution_and_mutation_but_not_readers() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let observer = Store::open(root.path()).unwrap();
        let mut session = store.create("demo", "goal", "step").unwrap();
        let start = store.history(&session).unwrap()[0].0;
        let owner = store
            .claim_execution(&session.id, Some("first instruction"))
            .unwrap();
        let before = observer.history(&session).unwrap();
        assert!(observer
            .claim_execution(&session.id, Some("losing instruction"))
            .is_err());
        assert!(observer.recover_session(&session.id).is_err());
        assert!(observer.fork(&mut session, start, "step").is_err());
        let branch = session.active_branch.clone();
        assert!(observer
            .switch_branch(&mut session, &branch, "step")
            .is_err());
        let plan =
            crate::restore::RestorePlan::prepare(root.path(), &observer, &session, start).unwrap();
        assert!(plan.apply_silent(root.path(), &observer, &session).is_err());
        assert_eq!(observer.session(&session.id).unwrap().status, "running");
        assert_eq!(observer.history(&session).unwrap(), before);
        let other = store.create("demo", "other goal", "step").unwrap();
        assert!(observer
            .claim_execution(&other.id, Some("losing other session instruction"))
            .is_err());
        assert_eq!(observer.session(&other.id).unwrap().status, "ready");
        assert!(!observer
            .history(&other)
            .unwrap()
            .iter()
            .any(|e| e.3 == "losing other session instruction"));
        store.finish_execution(&session.id, &owner, None).unwrap();
        drop(owner);
        let other_owner = observer.claim_execution(&other.id, None).unwrap();
        observer
            .finish_execution(&other.id, &other_owner, None)
            .unwrap();
        drop(other_owner);
        assert_eq!(store.session(&session.id).unwrap().status, "blocked");
        let next = store.claim_execution(&session.id, None).unwrap();
        store.finish_execution(&session.id, &next, None).unwrap();
    }

    #[test]
    fn abandoned_owner_requires_explicit_recovery_and_preserves_saved_grants() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let mut grants = crate::approvals::ApprovalStore::open(root.path()).unwrap();
        grants
            .grant(crate::approvals::Scope::Project, "shell", "cargo test")
            .unwrap();
        let owner = store.claim_execution(&session.id, None).unwrap();
        drop(owner); // Models unwinding or process death without normal cleanup.
        assert!(store.claim_execution(&session.id, None).is_err());
        assert!(store.lock_idle_session(&session.id).is_err());
        let recovered = store.recover_session(&session.id).unwrap();
        assert!(recovered.contains("unanswered approvals expired"));
        assert_eq!(store.session(&session.id).unwrap().status, "blocked");
        let grants = crate::approvals::ApprovalStore::open(root.path()).unwrap();
        assert!(grants.matched_scope("shell", "cargo test").is_some());
        assert!(store
            .recover_session(&session.id)
            .unwrap()
            .starts_with("No interrupted"));
        let next = store.claim_execution(&session.id, None).unwrap();
        store.finish_execution(&session.id, &next, None).unwrap();
    }

    #[test]
    fn legacy_running_flags_recover_but_completed_sessions_stay_completed() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("demo", "goal", "step").unwrap();
        session.status = "running".into();
        store.save(&session).unwrap();
        assert!(store.claim_execution(&session.id, None).is_err());
        store.recover_session(&session.id).unwrap();
        assert_eq!(store.session(&session.id).unwrap().status, "blocked");
        session.status = "completed".into();
        store.save(&session).unwrap();
        assert!(store
            .recover_session(&session.id)
            .unwrap()
            .starts_with("No interrupted"));
        assert!(store.claim_execution(&session.id, None).is_err());
        assert_eq!(store.session(&session.id).unwrap().status, "completed");
    }

    #[test]
    fn recovery_after_completion_does_not_repeat_the_completed_step() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("demo", "goal", "step").unwrap();
        let owner = store.claim_execution(&session.id, None).unwrap();
        session.status = "completed".into();
        store.save(&session).unwrap();
        drop(owner); // Death after completion was saved but before owner cleanup.
        store.recover_session(&session.id).unwrap();
        assert_eq!(store.session(&session.id).unwrap().status, "completed");
        assert!(store.claim_execution(&session.id, None).is_err());
    }

    #[test]
    fn interrupted_write_requires_owner_recovery_before_write_reconciliation() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let owner = store.claim_execution(&session.id, None).unwrap();
        store
            .db
            .execute_batch(
                "CREATE TRIGGER reject_journal BEFORE INSERT ON file_changes
            BEGIN SELECT RAISE(ABORT, 'injected journal failure'); END;",
            )
            .unwrap();
        assert!(store
            .apply_native_write(root.path(), &session, "step", "file.txt", None, "new")
            .is_err());
        drop(owner);
        store
            .db
            .execute_batch("DROP TRIGGER reject_journal;")
            .unwrap();
        assert!(store.recover_writes(root.path()).is_err());
        store.recover_session(&session.id).unwrap();
        assert!(store.claim_execution(&session.id, None).is_err());
        store.recover_writes(root.path()).unwrap();
        assert_eq!(store.changes(&session).unwrap().len(), 1);
        assert_eq!(
            fs::read_to_string(root.path().join("file.txt")).unwrap(),
            "new"
        );
        let next = store.claim_execution(&session.id, None).unwrap();
        store.finish_execution(&session.id, &next, None).unwrap();
    }

    #[test]
    fn killed_process_releases_lock_but_requires_recovery() {
        use std::{
            io::{BufRead, BufReader, Write},
            process::{Command, Stdio},
        };
        const FIXTURE: &str = "GREENLIGHT_EXECUTION_CRASH_FIXTURE";
        if let Some(path) = std::env::var_os(FIXTURE) {
            let store = Store::open(std::path::Path::new(&path)).unwrap();
            let session = store.list().unwrap().remove(0);
            let _owner = store.claim_execution(&session.id, None).unwrap();
            println!("OWNER-READY");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_line(&mut String::new()).unwrap();
            panic!("worker should have been killed");
        }
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "step").unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session_execution::tests::killed_process_releases_lock_but_requires_recovery",
                "--nocapture",
            ])
            .env(FIXTURE, root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = false;
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            if line.unwrap() == "OWNER-READY" {
                ready = true;
                break;
            }
        }
        if ready {
            assert!(store.claim_execution(&session.id, None).is_err());
            assert!(store.recover_session(&session.id).is_err());
            child.kill().unwrap();
        }
        let status = child.wait().unwrap();
        assert!(ready, "worker exited before ownership: {status}");
        assert!(!status.success());
        assert!(store.claim_execution(&session.id, None).is_err());
        store.recover_session(&session.id).unwrap();
        let next = store.claim_execution(&session.id, None).unwrap();
        store.finish_execution(&session.id, &next, None).unwrap();
    }

    #[tokio::test]
    async fn competing_cli_and_tui_resumes_cannot_clobber_the_owner() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: mock\n",
        )
        .unwrap();
        let folder = root.path().join("workflows/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: approve\nsteps: [approve]\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("approve.md"),
            "---\nid: approve\nkind: human\nverification:\n  type: none\n---\nApprove fixture\n",
        )
        .unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "approve").unwrap();
        let other_session = store
            .create("demo", "other session goal", "approve")
            .unwrap();
        let first = crate::tui_actions::resume(
            root.path().into(),
            session.id.clone(),
            "owner instruction".into(),
        );
        first
            .prompts
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let mut cli = crate::engine::Engine::open(root.path()).unwrap();
        assert!(cli
            .run(&session.id, Some("losing CLI instruction"), None)
            .await
            .is_err());
        let before_other = store.history(&other_session).unwrap();
        let error = cli
            .run(&other_session.id, Some("must not be sent"), None)
            .await
            .unwrap_err();
        assert!(error
            .downcast_ref::<crate::project_execution::ProjectConflict>()
            .is_some());
        assert_eq!(store.history(&other_session).unwrap(), before_other);
        assert!(store
            .usage_report(&other_session, crate::usage::Scope::Session)
            .unwrap()
            .calls
            .is_empty());
        let second = crate::tui_actions::resume(
            root.path().into(),
            session.id.clone(),
            "losing TUI instruction".into(),
        );
        assert!(second
            .finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .is_err());
        assert_eq!(store.session(&session.id).unwrap().status, "running");
        assert!(!store
            .history(&session)
            .unwrap()
            .iter()
            .any(|(_, _, _, text)| text.starts_with("losing")));
        first.answers.send("n".into()).unwrap();
        first
            .finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(store.session(&session.id).unwrap().status, "blocked");
        let fresh =
            crate::tui_actions::resume(root.path().into(), session.id.clone(), String::new());
        fresh
            .prompts
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        fresh.answers.send("y".into()).unwrap();
        fresh
            .finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(store.session(&session.id).unwrap().status, "completed");
    }

    #[tokio::test]
    async fn ordinary_engine_error_releases_owner_and_blocks_session() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: mock\n",
        )
        .unwrap();
        let mut engine = crate::engine::Engine::open(root.path()).unwrap();
        let session = engine
            .store
            .create("missing-workflow", "goal", "step")
            .unwrap();
        assert!(engine.run(&session.id, None, None).await.is_err());
        assert_eq!(engine.store.session(&session.id).unwrap().status, "blocked");
        let lock = engine.store.lock_idle_session(&session.id).unwrap();
        drop(lock);
        assert!(engine
            .store
            .history(&session)
            .unwrap()
            .iter()
            .any(|(_, _, kind, _)| kind == "runner.error"));
    }
}
