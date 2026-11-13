//! Durable intent before mutation; explicit reconciliation never rewrites files.
use crate::{
    restore::checked_path,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::{fs, io::Write, path::Path};
use uuid::Uuid;

struct Intent {
    id: String,
    session: String,
    branch: String,
    step: String,
    path: String,
    before: Option<String>,
    after: String,
}

fn snapshot(root: &Path, relative: &str) -> Result<Option<String>> {
    let path = checked_path(root, relative)?;
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl Store {
    pub(crate) fn native_write_lock(&self) -> Result<crate::locks::LockGuard> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join("native-write.lock"))?;
        let file = crate::locks::LockGuard::acquire(file)
            .context("another native write, recovery or restore is active")?;
        Ok(file)
    }

    pub fn ensure_no_pending_writes(&self) -> Result<()> {
        let count: i64 = self.db.query_row(
            "SELECT count(*) FROM write_intents WHERE status='pending'",
            [],
            |row| row.get(0),
        )?;
        if count != 0 {
            bail!("{count} unresolved native write(s); inspect history and run greenlight recover-writes before continuing");
        }
        Ok(())
    }

    fn prepare_write(
        &self,
        s: &Session,
        step: &str,
        path: &str,
        before: Option<&str>,
        after: &str,
    ) -> Result<String> {
        self.ensure_no_pending_writes()?;
        let id = Uuid::new_v4().to_string();
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO write_intents(id,session,branch,step,path,before_text,after_text,status)
            VALUES (?1,?2,?3,?4,?5,?6,?7,'pending')",
            params![id, s.id, s.active_branch, step, path, before, after],
        )?;
        self.event(s, Some(step), "file.write-intent", &format!("{id}: {path}"))?;
        tx.commit()?;
        Ok(id)
    }

    fn finish_write(&self, intent: &Intent, applied: bool) -> Result<()> {
        let mut session = self.session(&intent.session)?;
        session.active_branch.clone_from(&intent.branch);
        let tx = self.db.unchecked_transaction()?;
        if applied {
            self.add_change(
                &session,
                &intent.step,
                &intent.path,
                intent.before.as_deref(),
                &intent.after,
            )?;
        } else {
            self.event(
                &session,
                Some(&intent.step),
                "file.write-cancelled",
                &intent.id,
            )?;
        }
        let changed = tx.execute(
            "UPDATE write_intents SET status=?2 WHERE id=?1 AND status='pending'",
            params![intent.id, if applied { "completed" } else { "cancelled" }],
        )?;
        if changed != 1 {
            bail!("native write intent is no longer pending");
        }
        tx.commit()?;
        Ok(())
    }

    fn pending_writes(&self) -> Result<Vec<Intent>> {
        let mut query = self.db.prepare(
            "SELECT id,session,branch,step,path,before_text,after_text
            FROM write_intents WHERE status='pending' ORDER BY rowid",
        )?;
        let rows = query.query_map([], |row| {
            Ok(Intent {
                id: row.get(0)?,
                session: row.get(1)?,
                branch: row.get(2)?,
                step: row.get(3)?,
                path: row.get(4)?,
                before: row.get(5)?,
                after: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn apply_native_write(
        &self,
        root: &Path,
        s: &Session,
        step: &str,
        relative: &str,
        before: Option<&str>,
        after: &str,
    ) -> Result<()> {
        let _lock = self.native_write_lock()?;
        self.ensure_no_pending_writes()?;
        if snapshot(root, relative)?.as_deref() != before {
            bail!("file changed before native write; edit not applied");
        }
        let path = checked_path(root, relative)?;
        let id = self.prepare_write(s, step, relative, before, after)?;
        let intent = Intent {
            id,
            session: s.id.clone(),
            branch: s.active_branch.clone(),
            step: step.into(),
            path: relative.into(),
            before: before.map(str::to_owned),
            after: after.into(),
        };
        let parent = path.parent().context("write target has no parent")?;
        let temp = parent.join(format!(".greenlight-write-{}.tmp", intent.id));
        // No await between intent and completion. Cancellation cannot abandon a
        // live background filesystem operation after the mutation lock drops.
        let mutation: Result<()> = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            if let Ok(metadata) = fs::metadata(&path) {
                file.set_permissions(metadata.permissions())?;
            }
            file.write_all(after.as_bytes())?;
            file.sync_all()?;
            if snapshot(root, relative)?.as_deref() != before {
                bail!("file changed during native write; edit not applied");
            }
            fs::rename(&temp, &path)?;
            fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if let Err(error) = mutation {
            // Keep the intent: even an I/O error can occur after replacement.
            return Err(error.context(format!("native write {} interrupted; run greenlight recover-writes; temporary file may remain at {}", intent.id, temp.display())));
        }
        self.finish_write(&intent, true)
            .context("file changed but journal completion failed; run greenlight recover-writes")
    }

    pub fn recover_writes(&self, root: &Path) -> Result<Vec<String>> {
        let _lock = self.native_write_lock()?;
        let mut outcomes = Vec::new();
        for intent in self.pending_writes()? {
            let _session_lock = self.lock_idle_session(&intent.session)?;
            let current = match snapshot(root, &intent.path) {
                Ok(current) => current,
                Err(error) => {
                    outcomes.push(format!(
                        "Unresolved {} ({}): {error:#}",
                        intent.id, intent.path
                    ));
                    continue;
                }
            };
            let applied = if current.as_deref() == Some(&intent.after) {
                true
            } else if current == intent.before {
                false
            } else {
                outcomes.push(format!("Conflict {} ({}): contents match neither original nor intended write; preserve manual edits and reconcile explicitly", intent.id, intent.path));
                continue;
            };
            if applied {
                fs::File::open(checked_path(root, &intent.path)?)?.sync_all()?;
                fs::File::open(
                    checked_path(root, &intent.path)?
                        .parent()
                        .context("write target has no parent")?,
                )?
                .sync_all()?;
            }
            // A recovered session must not retain a stale running flag.
            // Saving it first is safe to repeat if journal completion fails.
            let mut session = self.session(&intent.session)?;
            session.status = "blocked".into();
            self.save(&session)?;
            self.finish_write(&intent, applied)?;
            outcomes.push(format!(
                "{} {} ({})",
                if applied { "Journaled" } else { "Cancelled" },
                intent.id,
                intent.path
            ));
        }
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_death_preserves_recoverable_intents() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        const FIXTURE: &str = "GREENLIGHT_WRITE_CRASH_FIXTURE";
        if let Some(root) = std::env::var_os(FIXTURE) {
            let root = Path::new(&root);
            let store = Store::open(root).unwrap();
            let _lock = store.native_write_lock().unwrap();
            let session = store.create("demo", "goal", "edit").unwrap();
            store
                .prepare_write(
                    &session,
                    "edit",
                    "file.txt",
                    Some("original"),
                    "replacement",
                )
                .unwrap();
            if std::env::var_os("GREENLIGHT_WRITE_CRASH_APPLIED").is_some() {
                fs::write(root.join("file.txt"), "replacement").unwrap();
            }
            println!("JOURNAL-READY");
            std::io::stdout().flush().unwrap();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            panic!("parent should kill the worker before it returns");
        }
        for applied in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("file.txt"), "original").unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "write_journal::tests::process_death_preserves_recoverable_intents",
                    "--nocapture",
                ])
                .env(FIXTURE, root.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
            if applied {
                command.env("GREENLIGHT_WRITE_CRASH_APPLIED", "1");
            } else {
                command.env_remove("GREENLIGHT_WRITE_CRASH_APPLIED");
            }
            let mut child = command.spawn().unwrap();
            let output = BufReader::new(child.stdout.take().unwrap());
            let mut ready = false;
            for line in output.lines() {
                if line.unwrap() == "JOURNAL-READY" {
                    ready = true;
                    break;
                }
            }
            if ready {
                child.kill().unwrap();
            }
            let status = child.wait().unwrap();
            assert!(ready, "worker exited before saving intent: {status}");
            assert!(!status.success());
            let store = Store::open(root.path()).unwrap();
            assert!(store.ensure_no_pending_writes().is_err());
            let session = store.list().unwrap().remove(0);
            store.recover_writes(root.path()).unwrap();
            assert_eq!(store.changes(&session).unwrap().len(), usize::from(applied));
            assert_eq!(
                snapshot(root.path(), "file.txt").unwrap().as_deref(),
                Some(if applied { "replacement" } else { "original" })
            );
        }
    }

    #[test]
    fn interrupted_writes_recover_before_and_after_replacement() {
        for applied in [false, true] {
            for original in [None, Some("original")] {
                let root = tempfile::tempdir().unwrap();
                let file = root.path().join("file.txt");
                if let Some(text) = original {
                    fs::write(&file, text).unwrap();
                }
                let session;
                {
                    let store = Store::open(root.path()).unwrap();
                    session = store.create("demo", "goal", "edit").unwrap();
                    store
                        .prepare_write(&session, "edit", "file.txt", original, "replacement")
                        .unwrap();
                    // Simulate process death on either side of atomic replacement.
                    if applied {
                        fs::write(&file, "replacement").unwrap();
                    }
                }
                let store = Store::open(root.path()).unwrap();
                assert!(store.ensure_no_pending_writes().is_err());
                assert!(store.changes(&session).unwrap().is_empty());
                store.recover_writes(root.path()).unwrap();
                store.ensure_no_pending_writes().unwrap();
                assert_eq!(store.changes(&session).unwrap().len(), usize::from(applied));
                assert_eq!(
                    snapshot(root.path(), "file.txt").unwrap().as_deref(),
                    if applied {
                        Some("replacement")
                    } else {
                        original
                    }
                );
                assert!(store.recover_writes(root.path()).unwrap().is_empty());
                if applied {
                    let change = &store.changes(&session).unwrap()[0];
                    assert_eq!(change.before.as_deref(), original);
                    assert_eq!(change.after, "replacement");
                    let start = store.history(&session).unwrap()[0].0;
                    crate::restore::RestorePlan::prepare(root.path(), &store, &session, start)
                        .unwrap()
                        .apply_silent(root.path(), &store, &session)
                        .unwrap();
                    assert_eq!(
                        snapshot(root.path(), "file.txt").unwrap().as_deref(),
                        original
                    );
                }
            }
        }
    }

    #[test]
    fn recovery_preserves_manual_edits_and_blocks_mutation_until_reconciled() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "edit").unwrap();
        store
            .prepare_write(
                &session,
                "edit",
                "file.txt",
                Some("original"),
                "replacement",
            )
            .unwrap();
        fs::write(root.path().join("file.txt"), "manual").unwrap();
        let outcomes = store.recover_writes(root.path()).unwrap();
        assert!(outcomes[0].starts_with("Conflict"));
        assert_eq!(
            fs::read_to_string(root.path().join("file.txt")).unwrap(),
            "manual"
        );
        assert!(store
            .apply_native_write(root.path(), &session, "edit", "other.txt", None, "other")
            .is_err());
        let start = store.history(&session).unwrap()[0].0;
        assert!(
            crate::restore::RestorePlan::prepare(root.path(), &store, &session, start).is_err()
        );
        let mut selected = session.clone();
        assert!(store.fork(&mut selected, start, "edit").is_err());
        assert!(store
            .switch_branch(&mut selected, &session.active_branch, "edit")
            .is_err());
        assert!(!root.path().join("other.txt").exists());
        fs::write(root.path().join("file.txt"), "original").unwrap();
        store.recover_writes(root.path()).unwrap();
        store.ensure_no_pending_writes().unwrap();
    }

    #[test]
    fn failed_intent_persistence_never_changes_source() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "edit").unwrap();
        fs::write(root.path().join("file.txt"), "original").unwrap();
        store
            .db
            .execute_batch(
                "CREATE TRIGGER reject_intent BEFORE INSERT ON write_intents
            BEGIN SELECT RAISE(ABORT, 'injected intent failure'); END;",
            )
            .unwrap();
        assert!(store
            .apply_native_write(
                root.path(),
                &session,
                "edit",
                "file.txt",
                Some("original"),
                "replacement"
            )
            .is_err());
        assert_eq!(
            snapshot(root.path(), "file.txt").unwrap().as_deref(),
            Some("original")
        );
        assert!(!store
            .history(&session)
            .unwrap()
            .iter()
            .any(|(_, _, k, _)| k == "file.write-intent"));
        store.ensure_no_pending_writes().unwrap();
    }

    #[test]
    fn failed_journal_completion_can_be_recovered_without_duplicate_edits() {
        let root = tempfile::tempdir().unwrap();
        let session;
        {
            let store = Store::open(root.path()).unwrap();
            session = store.create("demo", "goal", "edit").unwrap();
            store
                .db
                .execute_batch(
                    "CREATE TRIGGER reject_change BEFORE INSERT ON file_changes
                BEGIN SELECT RAISE(ABORT, 'injected journal failure'); END;",
                )
                .unwrap();
            assert!(store
                .apply_native_write(
                    root.path(),
                    &session,
                    "edit",
                    "file.txt",
                    None,
                    "replacement"
                )
                .is_err());
            assert_eq!(
                snapshot(root.path(), "file.txt").unwrap().as_deref(),
                Some("replacement")
            );
            assert!(store.changes(&session).unwrap().is_empty());
            assert!(!store
                .history(&session)
                .unwrap()
                .iter()
                .any(|(_, _, k, _)| k == "file.changed"));
            store
                .db
                .execute_batch("DROP TRIGGER reject_change;")
                .unwrap();
        }
        let store = Store::open(root.path()).unwrap();
        store.recover_writes(root.path()).unwrap();
        assert_eq!(store.changes(&session).unwrap().len(), 1);
        assert!(store.recover_writes(root.path()).unwrap().is_empty());
        assert_eq!(store.changes(&session).unwrap().len(), 1);
    }

    #[test]
    fn recovery_and_other_writes_cannot_race_an_active_writer() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let other = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "edit").unwrap();
        let lock = store.native_write_lock().unwrap();
        assert!(other.recover_writes(root.path()).is_err());
        assert!(other
            .apply_native_write(root.path(), &session, "edit", "file.txt", None, "new")
            .is_err());
        drop(lock);
        other
            .apply_native_write(root.path(), &session, "edit", "file.txt", None, "new")
            .unwrap();
        assert_eq!(other.changes(&session).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_does_not_follow_replaced_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "edit").unwrap();
        store
            .prepare_write(&session, "edit", "file.txt", None, "new")
            .unwrap();
        fs::write(root.path().join("manual.txt"), "new").unwrap();
        std::os::unix::fs::symlink("manual.txt", root.path().join("file.txt")).unwrap();
        assert!(store.recover_writes(root.path()).unwrap()[0].starts_with("Unresolved"));
        assert!(store.ensure_no_pending_writes().is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("manual.txt")).unwrap(),
            "new"
        );
    }
}
