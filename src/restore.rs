//! Guarded restoration of UTF-8 files captured by the native edit journal.
//! Never guesses changes made by shell commands or outside the harness.
use crate::{
    changes,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

pub struct RestorePlan {
    event: i64,
    history_cutoff: Option<i64>,
    items: Vec<RestoreItem>,
}

struct RestoreItem {
    relative: String,
    current: Option<String>,
    desired: Option<String>,
}

/// Reject traversal and all symlinks, including inside the project.
/// These checks are deliberately stricter than ordinary file reads/writes.
pub(crate) fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.as_os_str().is_empty()
        || p.is_absolute()
        || p.components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("unsafe restore path: {relative}");
    }
    let parts: Vec<_> = p.components().collect();
    if crate::paths::is_private_path(p) {
        bail!("internal or sensitive path cannot be restored: {relative}");
    }

    let mut candidate = root.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        candidate.push(part.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    bail!("refusing symlink in restore path: {relative}");
                }
                if i + 1 == parts.len() {
                    if !meta.is_file() {
                        bail!("restore target is not a regular file: {relative}");
                    }
                } else if !meta.is_dir() {
                    bail!("restore parent is not a directory: {relative}");
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if i + 1 != parts.len() {
                    bail!("restore parent directory does not exist: {relative}");
                }
            }
            Err(err) => return Err(err).with_context(|| format!("checking {relative}")),
        }
    }
    Ok(candidate)
}

/// Exact preimages are bounded; oversized files require manual handling.
pub(crate) fn snapshot(root: &Path, relative: &str) -> Result<Option<String>> {
    checked_path(root, relative)?;
    let target = crate::anchored_fs::Dir::open(root)?.target(Path::new(relative))?;
    snapshot_target(&target, relative)
}
pub(crate) fn snapshot_target(
    target: &crate::anchored_fs::Target,
    relative: &str,
) -> Result<Option<String>> {
    const LIMIT: u64 = 20 * 1024 * 1024;
    let file = match target.open() {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(None)
        }
        Err(error) => {
            return Err(error).with_context(|| format!("reading restore target {relative}"))
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        bail!("snapshot requires a regular file of at most 20 MiB: {relative}");
    }
    let mut data = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut data)?;
    if data.len() as u64 > LIMIT {
        bail!("snapshot exceeds 20 MiB: {relative}");
    }
    Ok(Some(String::from_utf8(data).with_context(|| {
        format!("non-UTF-8 file cannot be safely restored: {relative}")
    })?))
}

impl RestorePlan {
    pub(crate) fn checkpoint(&self) -> i64 {
        self.event
    }

    pub fn prepare(root: &Path, store: &Store, session: &Session, event: i64) -> Result<Self> {
        Self::prepare_paths(root, store, session, event, None)
    }

    /// A file revert must not be blocked by drift in unrelated files.
    pub fn prepare_paths(
        root: &Path,
        store: &Store,
        session: &Session,
        event: i64,
        paths: Option<&[String]>,
    ) -> Result<Self> {
        store.ensure_no_pending_writes()?;
        let history = store.history(session)?;
        if !history.iter().any(|(id, _, _, _)| *id == event) {
            bail!("event #{event} is not in the active branch ancestry");
        }

        // Desired is the state at the selected event. Expected is the state
        // after the latest recorded edit, including earlier explicit restores.
        // A gap between writes indicates an out-of-band change.
        let mut states: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
        for edit in store.workspace_edits(session)? {
            if paths.is_some_and(|paths| !paths.contains(&edit.path)) {
                continue;
            }
            let entry = states
                .entry(edit.path.clone())
                .or_insert_with(|| (edit.before.clone(), edit.before.clone()));
            if entry.1 != edit.before {
                bail!(
                    "tracked history for {} has a gap before event #{} (possible shell/manual edit); refusing restore",
                    edit.path, edit.event_id
                );
            }
            entry.1 = edit.after.clone();
            if edit.event_id <= event {
                entry.0 = edit.after;
            }
        }

        if let Some(paths) = paths {
            if paths.is_empty() || paths.iter().any(|path| !states.contains_key(path)) {
                bail!("revert selection must contain recorded file paths");
            }
        }

        // Preflight every tracked path before any mutation, even paths not
        // requiring an update at the requested historical event.
        let mut items = Vec::new();
        for (relative, (desired, expected)) in states {
            let current = snapshot(root, &relative)?;
            if current != expected {
                if current == desired {
                    // Already manually restored to the requested target.
                    continue;
                }
                bail!(
                    "refusing restore: {relative} differs from its latest tracked snapshot; preserve manual/shell edits first"
                );
            }
            if current != desired {
                items.push(RestoreItem {
                    relative,
                    current,
                    desired,
                });
            }
        }
        Ok(Self {
            event,
            items,
            history_cutoff: history.last().map(|e| e.0),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn preview_text(&self) -> String {
        let mut output = format!("Workspace restore preview at event #{}\n", self.event);
        if self.items.is_empty() {
            output.push_str("Tracked workspace files already match this event.\n");
        }
        for item in &self.items {
            let operation = match (&item.current, &item.desired) {
                (None, Some(_)) => "create",
                (Some(_), None) => "delete",
                _ => "modify",
            };
            output.push_str(&format!("\n[{operation}: {}]\n", item.relative));
            let mut diff = changes::unified(
                &item.relative,
                item.current.as_deref(),
                item.desired.as_deref().unwrap_or(""),
            );
            if item.desired.is_none() {
                diff = diff.replacen(&format!("+++ b/{}\n", item.relative), "+++ /dev/null\n", 1);
            }
            output.push_str(&diff);
        }
        output.push_str(
            "\nOnly recorded native writes and prior restores are covered. Shell/manual changes cannot be reconstructed.\nFiles are restored one at a time; a failure can leave a partial restore. Review workspace/history before retrying.\n"
        );
        output
    }

    pub fn preview(&self) {
        print!("{}", self.preview_text());
    }

    pub fn apply(self, root: &Path, store: &Store, session: &Session) -> Result<()> {
        self.apply_inner(root, store, session, true)
    }

    /// Use the same guarded per-file restore path while a TUI owns stdout.
    pub fn apply_silent(self, root: &Path, store: &Store, session: &Session) -> Result<()> {
        self.apply_inner(root, store, session, false)
    }

    fn apply_inner(
        self,
        root: &Path,
        store: &Store,
        session: &Session,
        verbose: bool,
    ) -> Result<()> {
        store.with_project_control(session, "restore", |lease| {
            self.apply_owned(root, store, session, verbose, lease)
        })
    }
    pub(crate) fn apply_owned(
        self,
        root: &Path,
        store: &Store,
        session: &Session,
        verbose: bool,
        lease: &crate::project_execution::ProjectLease,
    ) -> Result<()> {
        store.check_project(lease, session)?;
        if store.session(&session.id)?.active_branch != session.active_branch {
            bail!("session branch changed since restore preview");
        }
        if store.history(session)?.last().map(|e| e.0) != self.history_cutoff {
            bail!("restore cancelled: history changed since preview; review a fresh preview");
        }
        let _lock = store.native_write_lock()?;
        store.ensure_no_pending_writes()?;
        // Verify again after the user has examined the preview.
        for item in &self.items {
            if snapshot(root, &item.relative)? != item.current {
                bail!("restore cancelled: {} changed since preview", item.relative);
            }
        }

        let directory = crate::anchored_fs::Dir::open(root)?;
        // Filesystem changes and SQLite publication are not one transaction.
        // Keep successful earlier restores; compensating writes could overwrite
        // a new external edit or fail again. Never auto-retry a partial restore.
        let mut applied = 0;
        for item in self.items {
            let mut changed = false;
            let result: Result<()> = (|| {
                if snapshot(root, &item.relative)? != item.current {
                    bail!("{} changed during restore", item.relative);
                }
                checked_path(root, &item.relative)?;
                let target = directory.target(Path::new(&item.relative))?;
                if snapshot_target(&target, &item.relative)? != item.current {
                    bail!("{} changed during restore", item.relative);
                }
                target.check_parent(&directory, Path::new(&item.relative))?;
                match &item.desired {
                    Some(content) => target.atomic_write(content.as_bytes())?,
                    None => target.unlink()?,
                }
                changed = true;
                target.check_parent(&directory, Path::new(&item.relative))?;
                store.add_restoration(
                    session,
                    &item.relative,
                    item.current.as_deref(),
                    item.desired.as_deref(),
                )?;
                Ok(())
            })();
            if let Err(error) = result {
                let changed_count = applied + usize::from(changed);
                let message = format!("Restore failed after {changed_count} file(s) changed ({applied} journaled), at {}: {error:#}. Filesystem may be partially restored. Counts cover confirmed mutations; a failed filesystem operation may also have changed its target. Inspect workspace and history; do not retry blindly.",item.relative);
                let payload = serde_json::json!({"changed_files":changed_count,"journaled_files":applied,"path":item.relative,"error":message}).to_string();
                if let Err(log_error) =
                    store.event(session, None, "workspace.restore-failed", &payload)
                {
                    bail!("{message} Could not persist restore failure: {log_error:#}");
                }
                bail!("{message}");
            }
            applied += 1;
            if verbose {
                println!("[restored] {}", item.relative);
            }
        }
        store.event(session,None,"workspace.restore-completed",&format!("{applied} tracked file(s) updated"))
            .with_context(||format!("{applied} files restored and journaled, but completion could not be recorded; inspect history"))?;
        if verbose {
            println!("Restore complete: {applied} tracked file(s) updated.");
        }
        Ok(())
    }
}

impl Store {
    pub fn restore_and_fork(
        &self,
        root: &Path,
        session: &mut Session,
        plan: RestorePlan,
        event: i64,
        step: &str,
        verbose: bool,
    ) -> Result<()> {
        let previous = session.clone();
        self.with_project_control(&previous, "restart", |lease| {
            plan.apply_owned(root, self, session, verbose, lease)?;
            self.fork_owned(session, event, step, lease).context("Restore applied but fork publication failed; inspect workspace and history before retrying")
        })
    }
}

pub fn confirm(message: &str) -> Result<bool> {
    print!("{message} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_older_edit_and_record_the_restoration() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("test", "goal", "step").unwrap();
        let file = root.path().join("file.txt");
        store
            .add_change(&session, "step", "file.txt", Some("a\n"), "b\n")
            .unwrap();
        let first = store.history(&session).unwrap().last().unwrap().0;
        store
            .add_change(&session, "step", "file.txt", Some("b\n"), "c\n")
            .unwrap();
        fs::write(&file, "c\n").unwrap();
        let plan = RestorePlan::prepare(root.path(), &store, &session, first).unwrap();
        assert_eq!(plan.items.len(), 1);
        plan.apply(root.path(), &store, &session).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "b\n");
        assert!(RestorePlan::prepare(root.path(), &store, &session, first)
            .unwrap()
            .is_empty());
        assert_eq!(store.workspace_edits(&session).unwrap().len(), 3);
    }

    #[test]
    fn restoration_removes_newly_created_file() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("test", "goal", "step").unwrap();
        let start = store.history(&session).unwrap()[0].0;
        store
            .add_change(&session, "step", "new.txt", None, "created\n")
            .unwrap();
        fs::write(root.path().join("new.txt"), "created\n").unwrap();
        let plan = RestorePlan::prepare(root.path(), &store, &session, start).unwrap();
        plan.apply(root.path(), &store, &session).unwrap();
        assert!(!root.path().join("new.txt").exists());
        assert!(store
            .workspace_edits(&session)
            .unwrap()
            .last()
            .unwrap()
            .after
            .is_none());
    }

    #[test]
    fn refuses_manual_changes_without_touching_files() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("test", "goal", "step").unwrap();
        let start = store.history(&session).unwrap()[0].0;
        store
            .add_change(&session, "step", "file.txt", Some("a\n"), "b\n")
            .unwrap();
        fs::write(root.path().join("file.txt"), "manual\n").unwrap();
        assert!(RestorePlan::prepare(root.path(), &store, &session, start).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("file.txt")).unwrap(),
            "manual\n"
        );
    }

    #[test]
    fn refuses_symlink_and_traversal() {
        let root = tempfile::tempdir().unwrap();
        assert!(checked_path(root.path(), "../outside").is_err());
        assert!(checked_path(root.path(), ".stepwright/state/history.db").is_err());
        assert!(checked_path(root.path(), ".greenlight/state/history.db").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.path(), root.path().join("link")).unwrap();
            assert!(checked_path(root.path(), "link/file.txt").is_err());
        }
    }

    #[test]
    fn restore_then_fork_preserves_selected_event_state() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("test", "goal", "step").unwrap();
        let first = store
            .add_change(&session, "step", "a.txt", Some("old"), "one")
            .unwrap();
        store
            .add_change(&session, "step", "a.txt", Some("one"), "two")
            .unwrap();
        fs::write(root.path().join("a.txt"), "two").unwrap();
        RestorePlan::prepare(root.path(), &store, &session, first)
            .unwrap()
            .apply(root.path(), &store, &session)
            .unwrap();
        store.fork(&mut session, first, "step").unwrap();
        assert!(RestorePlan::prepare(root.path(), &store, &session, first)
            .unwrap()
            .is_empty());
        assert_eq!(
            fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "one"
        );
    }
    #[test]
    fn failed_completion_publication_keeps_journaled_files_and_does_not_fork() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("test", "goal", "step").unwrap();
        let checkpoint = store.history(&session).unwrap()[0].0;
        let branch = session.active_branch.clone();
        for path in ["a.txt", "b.txt"] {
            store
                .add_change(&session, "step", path, Some("old"), "new")
                .unwrap();
            fs::write(root.path().join(path), "new").unwrap();
        }
        let plan = RestorePlan::prepare(root.path(), &store, &session, checkpoint).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_restore_completion BEFORE INSERT ON events WHEN NEW.kind='workspace.restore-completed' BEGIN SELECT RAISE(ABORT,'injected completion failure'); END;").unwrap();
        let error = store
            .restore_and_fork(root.path(), &mut session, plan, checkpoint, "step", false)
            .unwrap_err();
        assert!(format!("{error:#}")
            .contains("2 files restored and journaled, but completion could not be recorded"));
        for path in ["a.txt", "b.txt"] {
            assert_eq!(fs::read_to_string(root.path().join(path)).unwrap(), "old");
        }
        assert_eq!(store.workspace_edits(&session).unwrap().len(), 4);
        assert_eq!(session.active_branch, branch);
        assert_eq!(store.session(&session.id).unwrap().active_branch, branch);
        let history = store.history(&session).unwrap();
        assert_eq!(
            history
                .iter()
                .filter(|e| e.2 == "workspace.restored")
                .count(),
            2
        );
        assert!(!history.iter().any(|e| matches!(
            e.2.as_str(),
            "workspace.restore-completed" | "branch.forked"
        )));
    }

    #[test]
    fn snapshots_reject_large_files_without_changing_them() {
        let root = tempfile::tempdir().unwrap();
        let file = fs::File::create(root.path().join("large.txt")).unwrap();
        file.set_len(1024 * 1024 * 1024).unwrap();
        assert!(snapshot(root.path(), "large.txt")
            .unwrap_err()
            .to_string()
            .contains("20 MiB"));
        assert_eq!(file.metadata().unwrap().len(), 1024 * 1024 * 1024);
    }
}
