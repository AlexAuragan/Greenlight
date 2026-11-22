//! One project owner spans inference, user waits and control operations.
use crate::{
    locks::LockGuard,
    store::{Session, Store},
};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use std::{fmt, path::Path, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ProjectConflict {
    pub interrupted: bool,
    pub session: Option<Session>,
    pub operation: String,
}
impl fmt::Display for ProjectConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.interrupted {
            "Project execution interrupted"
        } else {
            "Project busy"
        };
        write!(f, "{state}")?;
        if let Some(s) = &self.session {
            write!(
                f,
                " — {} · step {} · {} · session {}",
                s.goal.chars().take(160).collect::<String>(),
                s.current_step,
                self.operation,
                s.id
            )?;
            if self.interrupted {
                write!(f, ". Inspect history, then run greenlight recover-session {}. Recovery does not resume work", s.id)?;
            } else {
                write!(f, ". Open the owning session; stop its worker and wait for it to finish before starting another. Exact wait state may be unavailable in this client")?;
            }
        } else {
            write!(
                f,
                " — project control operation in progress; refresh before trying again"
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for ProjectConflict {}

#[derive(Clone, Debug)]
pub(crate) struct ProjectRecord {
    pub token: String,
    pub session: String,
    pub branch: String,
    pub operation: String,
}
/// Constructed only after acquiring the project OS lock. A weak capability in the
/// owning Store lets native writes reuse this lease, never another Store's lease.
pub(crate) struct ProjectLease {
    _lock: LockGuard,
    pub record: ProjectRecord,
}

impl Store {
    pub(crate) fn project_record(&self) -> Result<Option<ProjectRecord>> {
        self.db
            .query_row(
                "SELECT token,session,branch,operation FROM project_owner WHERE singleton=1",
                [],
                |r| {
                    Ok(ProjectRecord {
                        token: r.get(0)?,
                        session: r.get(1)?,
                        branch: r.get(2)?,
                        operation: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
    /// Keep probes from briefly rejecting a real execution claim. This guard
    /// never spans workflow execution, and is always acquired before SQLite work.
    fn project_observation_lock(&self) -> Result<LockGuard> {
        if !self.db.is_autocommit() {
            bail!("Project ownership must be acquired before a SQLite transaction");
        }
        Ok(LockGuard::wait(
            self.directory_fd
                .lock_file(Path::new("project-observation.lock"))?,
        )?)
    }
    pub(crate) fn project_lock(&self) -> Result<LockGuard> {
        let _observation = self.project_observation_lock()?;
        self.try_project_lock()
    }
    /// Caller holds the observation guard until its probe or lock acquisition ends.
    fn try_project_lock(&self) -> Result<LockGuard> {
        let file = self
            .directory_fd
            .lock_file(Path::new("project-execution.lock"))?;
        match LockGuard::acquire(file) {
            Ok(lock) => Ok(lock),
            Err(std::fs::TryLockError::WouldBlock) => Err(self
                .project_conflict(false, self.project_record()?.as_ref())?
                .into()),
            Err(error) => Err(error.into()),
        }
    }
    fn project_conflict(
        &self,
        interrupted: bool,
        record: Option<&ProjectRecord>,
    ) -> Result<ProjectConflict> {
        Ok(ProjectConflict {
            interrupted,
            session: record.map(|r| self.session(&r.session)).transpose()?,
            operation: record
                .map(|r| r.operation.clone())
                .unwrap_or_else(|| "control".into()),
        })
    }
    fn legacy_project_owner(&self) -> Result<Option<(Session, usize)>> {
        let id: Option<String> = self.db.query_row("SELECT id FROM sessions WHERE status='running' OR EXISTS(SELECT 1 FROM execution_owners WHERE session=sessions.id) ORDER BY id LIMIT 1",[],|r|r.get(0)).optional()?;
        id.map(|id| {
            let count = self.db.query_row("SELECT count(*) FROM sessions WHERE status='running' OR EXISTS(SELECT 1 FROM execution_owners WHERE session=sessions.id)",[],|r|r.get::<_,usize>(0))?;
            Ok((self.session(&id)?,count))
        }).transpose()
    }
    pub fn ensure_project_available(&self) -> Result<()> {
        if let Some(conflict) = self.project_status()? {
            return Err(conflict.into());
        }
        Ok(())
    }
    pub fn project_status(&self) -> Result<Option<ProjectConflict>> {
        let _observation = self.project_observation_lock()?;
        let lock = match self.try_project_lock() {
            Ok(lock) => lock,
            Err(error) if error.downcast_ref::<ProjectConflict>().is_some() => {
                return Ok(Some(error.downcast::<ProjectConflict>()?))
            }
            Err(error) => return Err(error),
        };
        let status = if let Some(record) = self.project_record()? {
            Some(self.project_conflict(true, Some(&record))?)
        } else if let Some((session, count)) = self.legacy_project_owner()? {
            let interrupted = self.execution_state(&session.id)?
                != crate::session_execution::ExecutionState::Live;
            Some(ProjectConflict {
                interrupted,
                session: Some(session),
                operation: format!(
                    "legacy execution ({count} unresolved owner(s); recover each explicitly)"
                ),
            })
        } else {
            None
        };
        drop(lock);
        Ok(status)
    }
    /// Acquires only the OS lease; callers publish it with their state transaction.
    pub(crate) fn acquire_project(
        &self,
        s: &Session,
        operation: &str,
    ) -> Result<Arc<ProjectLease>> {
        let lock = self.project_lock()?;
        if let Some(record) = self.project_record()? {
            return Err(self.project_conflict(true, Some(&record))?.into());
        }
        if let Some((session, count)) = self.legacy_project_owner()? {
            let interrupted = self.execution_state(&session.id)?
                != crate::session_execution::ExecutionState::Live;
            return Err(ProjectConflict {
                interrupted,
                session: Some(session),
                operation: format!(
                    "legacy execution ({count} unresolved owner(s); recover each explicitly)"
                ),
            }
            .into());
        }
        Ok(Arc::new(ProjectLease {
            _lock: lock,
            record: ProjectRecord {
                token: Uuid::new_v4().to_string(),
                session: s.id.clone(),
                branch: s.active_branch.clone(),
                operation: operation.into(),
            },
        }))
    }
    pub(crate) fn publish_project(&self, lease: &ProjectLease) -> Result<()> {
        let r = &lease.record;
        self.db.execute("INSERT INTO project_owner(singleton,token,session,branch,operation,pid) VALUES(1,?1,?2,?3,?4,?5)", params![r.token,r.session,r.branch,r.operation,std::process::id()])?;
        Ok(())
    }
    pub(crate) fn release_project(&self, lease: &ProjectLease) -> Result<()> {
        if self.db.execute(
            "DELETE FROM project_owner WHERE singleton=1 AND token=?1",
            [&lease.record.token],
        )? != 1
        {
            bail!("Project ownership changed unexpectedly; inspect history and recover the owner");
        }
        Ok(())
    }
    pub(crate) fn check_project(&self, lease: &ProjectLease, s: &Session) -> Result<()> {
        if lease.record.session != s.id
            || lease.record.branch != s.active_branch
            || self.session(&s.id)?.active_branch != s.active_branch
            || self
                .project_record()?
                .is_none_or(|r| r.token != lease.record.token)
        {
            bail!("Project/session/branch ownership changed; inspect history before continuing");
        }
        Ok(())
    }
    /// Short control scopes publish durable crash evidence, but do not mark a
    /// workflow Running. Validation errors release normally; unwinds leave recovery evidence.
    pub(crate) fn with_project_control<T>(
        &self,
        s: &Session,
        operation: &str,
        action: impl FnOnce(&ProjectLease) -> Result<T>,
    ) -> Result<T> {
        let lease = self.acquire_project(s, operation)?;
        let _session = self.lock_idle_session(&s.id)?;
        if self.session(&s.id)?.active_branch != s.active_branch {
            bail!("Session branch changed before project control");
        }
        self.publish_project(&lease)?;
        let result = action(&lease);
        let cleanup = self.release_project(&lease);
        match (result, cleanup) {
            (result, Ok(())) => result,
            (Ok(_), Err(error)) => {
                Err(error.context("Project cleanup failed; explicit owner recovery required"))
            }
            (Err(error), Err(cleanup)) => Err(error.context(format!(
                "Project cleanup also failed: {cleanup:#}; explicit owner recovery required"
            ))),
        }
    }
    pub(crate) fn owned_native_write<T>(
        &self,
        s: &Session,
        action: impl FnOnce(&ProjectLease) -> Result<T>,
    ) -> Result<T> {
        let lease = self.active_project.borrow().upgrade();
        if let Some(lease) = lease {
            self.check_project(&lease, s)?;
            action(&lease)
        } else {
            self.with_project_control(s, "native-write", action)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn conflict(error: anyhow::Error, interrupted: bool) -> ProjectConflict {
        let conflict = error.downcast::<ProjectConflict>().unwrap();
        assert_eq!(conflict.interrupted, interrupted);
        conflict
    }
    #[test]
    fn live_owner_excludes_other_sessions_controls_and_stores_but_not_readers() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let observer = Store::open(root.path()).unwrap();
        let a = store.create("demo", "Owning goal", "work").unwrap();
        let mut b = store.create("demo", "Other goal", "work").unwrap();
        let before = observer.history(&b).unwrap();
        let cutoff = before[0].0;
        let branch = b.active_branch.clone();
        let plan =
            crate::restore::RestorePlan::prepare(root.path(), &observer, &b, cutoff).unwrap();
        let owner = store.claim_execution(&a.id, None).unwrap();
        let busy = conflict(
            observer
                .claim_execution(&b.id, Some("must not be recorded"))
                .err()
                .unwrap(),
            false,
        );
        assert_eq!(busy.session.unwrap().id, a.id);
        assert!(observer.claim_tool_execution(&b.id, &branch).is_err());
        assert!(observer.fork(&mut b, cutoff, "work").is_err());
        assert!(observer.switch_branch(&mut b, &branch, "work").is_err());
        assert!(plan.apply_silent(root.path(), &observer, &b).is_err());
        assert!(observer.recover_writes(root.path()).is_err());
        assert!(observer.recover_session(&b.id).is_err());
        assert!(observer
            .apply_native_write(root.path(), &b, "work", "file.txt", None, "bad")
            .is_err());
        assert!(store
            .apply_native_write(root.path(), &b, "work", "file.txt", None, "bad")
            .is_err());
        assert!(!root.path().join("file.txt").exists());
        assert_eq!(observer.history(&b).unwrap(), before);
        assert_eq!(observer.session(&b.id).unwrap().status, "ready");
        assert!(observer
            .usage_report(&b, crate::usage::Scope::Session)
            .unwrap()
            .calls
            .is_empty());
        assert!(crate::conversation_dump::Dump::load(&observer, &b.id, None, false).is_ok());
        observer
            .event(
                &b,
                None,
                "manager.message",
                r#"{"text":"read-only question"}"#,
            )
            .unwrap();
        assert!(observer
            .history(&b)
            .unwrap()
            .iter()
            .any(|e| e.2 == "manager.message"));
        store
            .apply_native_write(
                root.path(),
                &store.session(&a.id).unwrap(),
                "work",
                "owner.txt",
                None,
                "valid",
            )
            .unwrap();
        store.finish_execution(&a.id, &owner, None).unwrap();
        // The lock is still held until worker guards actually drop, even though cleanup committed.
        conflict(observer.claim_execution(&b.id, None).err().unwrap(), false);
        drop(owner);
        let next = observer.claim_execution(&b.id, None).unwrap();
        observer.finish_execution(&b.id, &next, None).unwrap();
    }
    #[test]
    fn independent_projects_can_execute_simultaneously() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let one = Store::open(a.path()).unwrap();
        let two = Store::open(b.path()).unwrap();
        let sa = one.create("demo", "one", "work").unwrap();
        let sb = two.create("demo", "two", "work").unwrap();
        let oa = one.claim_execution(&sa.id, None).unwrap();
        let ob = two.claim_execution(&sb.id, None).unwrap();
        one.finish_execution(&sa.id, &oa, None).unwrap();
        two.finish_execution(&sb.id, &ob, None).unwrap();
    }
    #[test]
    fn interrupted_project_blocks_every_session_until_the_matching_owner_is_recovered() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let a = store.create("demo", "one", "work").unwrap();
        let b = store.create("demo", "two", "work").unwrap();
        drop(store.claim_execution(&a.id, None).unwrap());
        let stopped = conflict(store.claim_execution(&b.id, None).err().unwrap(), true);
        assert_eq!(stopped.session.unwrap().id, a.id);
        assert!(store.recover_session(&b.id).is_err());
        store.recover_session(&a.id).unwrap();
        let owner = store.claim_execution(&b.id, None).unwrap();
        store.finish_execution(&b.id, &owner, None).unwrap();
    }
    #[test]
    fn abandoned_maintenance_is_recoverable_without_a_running_session_or_execution_row() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "work").unwrap();
        s.status = "completed".into();
        store.save(&s).unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<()> =
                store.with_project_control(&s, "restore", |_| panic!("simulated control crash"));
        }))
        .is_err());
        assert_eq!(
            store.project_status().unwrap().unwrap().operation,
            "restore"
        );
        assert_eq!(
            store.execution_state(&s.id).unwrap(),
            crate::session_execution::ExecutionState::Interrupted
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM execution_owners", [], |r| r
                    .get::<_, usize>(0))
                .unwrap(),
            0
        );
        store.recover_session(&s.id).unwrap();
        assert_eq!(store.session(&s.id).unwrap().status, "completed");
        assert!(store.project_status().unwrap().is_none());
    }
    #[test]
    fn claim_and_cleanup_failures_preserve_atomic_owner_state() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let before = store.history(&s).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_start BEFORE INSERT ON events WHEN NEW.kind='runner.started' BEGIN SELECT RAISE(ABORT,'injected start failure'); END;").unwrap();
        assert!(store.claim_execution(&s.id, Some("not recorded")).is_err());
        assert!(store.project_record().unwrap().is_none());
        assert_eq!(store.history(&s).unwrap(), before);
        assert_eq!(store.session(&s.id).unwrap().status, "ready");
        store
            .db
            .execute_batch("DROP TRIGGER reject_start;")
            .unwrap();
        let owner = store.claim_execution(&s.id, None).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_cleanup BEFORE DELETE ON project_owner BEGIN SELECT RAISE(ABORT,'injected cleanup failure'); END;").unwrap();
        assert!(store.finish_execution(&s.id, &owner, None).is_err());
        assert_eq!(store.session(&s.id).unwrap().status, "running");
        assert!(store.project_record().unwrap().is_some());
        drop(owner);
        assert!(store.recover_session(&s.id).is_err());
        assert!(store.project_record().unwrap().is_some());
        store
            .db
            .execute_batch("DROP TRIGGER reject_cleanup;")
            .unwrap();
        store.recover_session(&s.id).unwrap();
        assert!(store.project_record().unwrap().is_none());
    }
    #[test]
    fn restore_and_fork_share_a_lease_and_preserve_partial_failure_evidence() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let other = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "work").unwrap();
        let b = store.create("demo", "other", "work").unwrap();
        let checkpoint = store.history(&s).unwrap()[0].0;
        store
            .apply_native_write(root.path(), &s, "work", "a.txt", None, "edit")
            .unwrap();
        let plan =
            crate::restore::RestorePlan::prepare(root.path(), &store, &s, checkpoint).unwrap();
        let initial = s.clone();
        store
            .with_project_control(&initial, "restart", |lease| {
                assert!(other.claim_execution(&b.id, None).is_err());
                plan.apply_owned(root.path(), &store, &s, false, lease)?;
                assert!(other.claim_execution(&b.id, None).is_err());
                store.fork_owned(&mut s, checkpoint, "work", lease)
            })
            .unwrap();
        assert_ne!(s.active_branch, initial.active_branch);
        assert!(!root.path().join("a.txt").exists());
        store
            .apply_native_write(root.path(), &s, "work", "a.txt", None, "new edit")
            .unwrap();
        let plan =
            crate::restore::RestorePlan::prepare(root.path(), &store, &s, checkpoint).unwrap();
        let branch = s.active_branch.clone();
        store.db.execute_batch("CREATE TRIGGER reject_fork BEFORE INSERT ON branches BEGIN SELECT RAISE(ABORT,'injected fork failure'); END;").unwrap();
        let error = store
            .restore_and_fork(root.path(), &mut s, plan, checkpoint, "work", false)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("Restore applied but fork publication failed"));
        assert_eq!(store.session(&s.id).unwrap().active_branch, branch);
        assert!(!root.path().join("a.txt").exists());
        assert!(store
            .history(&s)
            .unwrap()
            .iter()
            .any(|e| e.2 == "workspace.restore-completed"));
    }
    #[test]
    fn concurrent_claimants_in_one_process_have_one_winner() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let sessions = [
            store.create("demo", "one", "work").unwrap(),
            store.create("demo", "two", "work").unwrap(),
        ];
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles = sessions
            .into_iter()
            .map(|session| {
                let root = root.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let store = Store::open(&root).unwrap();
                    barrier.wait();
                    let claim = store.claim_execution(&session.id, None);
                    barrier.wait(); // winner retains the guards until the other claim finishes
                    match claim {
                        Ok(owner) => {
                            store.finish_execution(&session.id, &owner, None).unwrap();
                            true
                        }
                        Err(error) => {
                            conflict(error, false);
                            false
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| usize::from(h.join().unwrap()))
                .sum::<usize>(),
            1
        );
    }
    #[test]
    fn process_claim_is_exclusive_and_crash_requires_cross_session_recovery() {
        use std::{
            io::{BufRead, BufReader, Write},
            process::{Command, Stdio},
        };
        const ROOT: &str = "GREENLIGHT_PROJECT_CRASH_ROOT";
        if let Some(root) = std::env::var_os(ROOT) {
            let store = Store::open(Path::new(&root)).unwrap();
            let id = std::env::var("GREENLIGHT_PROJECT_CRASH_SESSION").unwrap();
            let _owner = store.claim_execution(&id, None).unwrap();
            println!("PROJECT-READY");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_line(&mut String::new()).unwrap();
            panic!("parent should kill the owner");
        }
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let a = store.create("demo", "owner", "work").unwrap();
        let b = store.create("demo", "blocked", "work").unwrap();
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","project_execution::tests::process_claim_is_exclusive_and_crash_requires_cross_session_recovery","--nocapture"]).env(ROOT,root.path()).env("GREENLIGHT_PROJECT_CRASH_SESSION",&a.id).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let ready = BufReader::new(child.stdout.take().unwrap())
            .lines()
            .any(|line| line.unwrap() == "PROJECT-READY");
        if ready {
            conflict(store.claim_execution(&b.id, None).err().unwrap(), false);
            child.kill().unwrap();
        }
        let status = child.wait().unwrap();
        assert!(ready, "child failed before ownership: {status}");
        conflict(store.claim_execution(&b.id, None).err().unwrap(), true);
        assert_eq!(store.session(&b.id).unwrap().status, "ready");
        store.recover_session(&a.id).unwrap();
        let owner = store.claim_execution(&b.id, None).unwrap();
        store.finish_execution(&b.id, &owner, None).unwrap();
    }
    #[test]
    fn legacy_owner_in_another_session_blocks_execution_and_control() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut a = store.create("demo", "legacy", "work").unwrap();
        let mut b = store.create("demo", "new", "work").unwrap();
        a.status = "running".into();
        store.save(&a).unwrap();
        conflict(store.claim_execution(&b.id, None).err().unwrap(), true);
        let cutoff = store.history(&b).unwrap()[0].0;
        assert!(store.fork(&mut b, cutoff, "work").is_err());
        store.recover_session(&a.id).unwrap();
        let owner = store.claim_execution(&b.id, None).unwrap();
        store.finish_execution(&b.id, &owner, None).unwrap();
    }
    #[test]
    fn project_claim_is_rejected_inside_a_sqlite_transaction() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let _tx = store.db.unchecked_transaction().unwrap();
        assert!(store
            .claim_execution(&s.id, None)
            .err()
            .unwrap()
            .to_string()
            .contains("before a SQLite transaction"));
    }
    #[test]
    fn observation_probe_delays_a_process_claim_without_rejecting_or_admitting_a_second_owner() {
        use std::{
            io::{BufRead, BufReader, Write},
            process::{Command, Stdio},
            sync::mpsc,
            time::Duration,
        };
        const ROOT: &str = "GREENLIGHT_PROJECT_PROBE_ROOT";
        if let Some(root) = std::env::var_os(ROOT) {
            let store = Store::open(Path::new(&root)).unwrap();
            let session = std::env::var("GREENLIGHT_PROJECT_PROBE_SESSION").unwrap();
            println!("PROBE-ATTEMPTING");
            std::io::stdout().flush().unwrap();
            let owner = store.claim_execution(&session, None).unwrap();
            println!("PROBE-CLAIMED");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_line(&mut String::new()).unwrap();
            store.finish_execution(&session, &owner, None).unwrap();
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let observer = Store::open(root.path()).unwrap();
        let session = observer.create("demo", "Worker", "work").unwrap();
        let other = observer.create("demo", "Other", "work").unwrap();
        // Freeze precisely the old failing interleaving: an idle observation
        // holds the execution probe while a separate process starts a worker.
        let observation = observer.project_observation_lock().unwrap();
        let probe = observer.try_project_lock().unwrap();
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","project_execution::tests::observation_probe_delays_a_process_claim_without_rejecting_or_admitting_a_second_owner","--nocapture"]).env(ROOT,root.path()).env("GREENLIGHT_PROJECT_PROBE_SESSION",&session.id).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            assert!(output.read_line(&mut line).unwrap() > 0);
            if line.contains("PROBE-ATTEMPTING") {
                break;
            }
        }
        let (sent, received) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in output.lines() {
                if line.unwrap().contains("PROBE-CLAIMED") {
                    sent.send(()).unwrap();
                }
            }
        });
        let blocked = matches!(
            received.recv_timeout(Duration::from_millis(40)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(observer.project_record().unwrap().is_none());
        drop(probe);
        drop(observation);
        let claimed = received.recv_timeout(Duration::from_secs(5)).is_ok();
        if claimed {
            let status = observer.project_status().unwrap().unwrap();
            assert!(!status.interrupted);
            assert_eq!(status.session.unwrap().id, session.id);
            conflict(
                observer.claim_execution(&other.id, None).err().unwrap(),
                false,
            );
        }
        // Always unblock/reap the fixture even when its claim failed.
        let _ = child.stdin.as_mut().unwrap().write_all(b"\n");
        let exit = child.wait().unwrap();
        reader.join().unwrap();
        assert!(
            blocked && claimed && exit.success(),
            "Worker failed after observation finished"
        );
        assert!(observer.project_status().unwrap().is_none());
        let owner = observer.claim_execution(&other.id, None).unwrap();
        observer.finish_execution(&other.id, &owner, None).unwrap();
    }

    #[test]
    fn observation_lock_order_rejects_an_open_sqlite_transaction() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let tx = store.db.unchecked_transaction().unwrap();
        assert!(store.project_status().is_err());
        assert!(store.project_lock().is_err());
        tx.rollback().unwrap();
        assert!(store.project_status().unwrap().is_none());
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;
    #[test]
    fn multiple_legacy_owners_are_explicit_and_recovery_never_clears_unrelated_sessions() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut a = store.create("demo", "one", "work").unwrap();
        let mut b = store.create("demo", "two", "work").unwrap();
        let c = store.create("demo", "new", "work").unwrap();
        for session in [&mut a, &mut b] {
            session.status = "running".into();
            store.save(session).unwrap();
        }
        let conflict = store.project_status().unwrap().unwrap();
        assert!(conflict.interrupted);
        assert!(conflict.operation.contains("2 unresolved owner(s)"));
        assert!(store.claim_execution(&c.id, None).is_err());
        store.recover_session(&a.id).unwrap();
        assert_eq!(store.session(&b.id).unwrap().status, "running");
        assert!(store.claim_execution(&c.id, None).is_err());
        store.recover_session(&b.id).unwrap();
        let owner = store.claim_execution(&c.id, None).unwrap();
        store.finish_execution(&c.id, &owner, None).unwrap();
    }
    #[test]
    fn a_live_legacy_session_is_reported_busy_and_cannot_be_recovered() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let observer = Store::open(root.path()).unwrap();
        let mut a = store.create("demo", "legacy", "work").unwrap();
        let b = store.create("demo", "new", "work").unwrap();
        a.status = "running".into();
        store.save(&a).unwrap();
        let lock = store.session_lock(&a.id).unwrap();
        let error = observer
            .claim_execution(&b.id, None)
            .err()
            .unwrap()
            .downcast::<ProjectConflict>()
            .unwrap();
        assert!(!error.interrupted);
        assert!(observer.recover_session(&a.id).is_err());
        assert_eq!(store.session(&a.id).unwrap().status, "running");
        drop(lock);
        observer.recover_session(&a.id).unwrap();
    }
}
