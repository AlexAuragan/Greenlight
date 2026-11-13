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
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub struct RestorePlan {
    event: i64,
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
        || p.components().any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("unsafe restore path: {relative}");
    }
    let parts: Vec<_> = p.components().collect();
    if parts.iter().any(|part| {
        let name = part.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | ".env" | ".ssh")
    }) || crate::paths::is_internal_state(p)
    {
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

fn snapshot(root: &Path, relative: &str) -> Result<Option<String>> {
    let path = checked_path(root, relative)?;
    match fs::read(path) {
        Ok(data) => Ok(Some(String::from_utf8(data).with_context(|| {
            format!("non-UTF-8 file cannot be safely restored: {relative}")
        })?)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading restore target {relative}")),
    }
}

impl RestorePlan {
    pub fn prepare(root: &Path, store: &Store, session: &Session, event: i64) -> Result<Self> {
        store.ensure_no_pending_writes()?;
        if !store.history(session)?.iter().any(|(id, _, _, _)| *id == event) {
            bail!("event #{event} is not in the active branch ancestry");
        }

        // Desired is the state at the selected event. Expected is the state
        // after the latest recorded edit, including earlier explicit restores.
        // A gap between writes indicates an out-of-band change.
        let mut states: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
        for edit in store.workspace_edits(session)? {
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
                items.push(RestoreItem { relative, current, desired });
            }
        }
        Ok(Self { event, items })
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
                &item.relative, item.current.as_deref(), item.desired.as_deref().unwrap_or(""),
            );
            if item.desired.is_none() {
                diff = diff.replacen(
                    &format!("+++ b/{}\n", item.relative), "+++ /dev/null\n", 1,
                );
            }
            output.push_str(&diff);
        }
        output.push_str(
            "\nOnly recorded native writes and prior restores are covered. Shell/manual changes cannot be reconstructed.\n"
        );
        output
    }

    pub fn preview(&self) {
        print!("{}", self.preview_text());
    }

    pub fn apply(self, root: &Path, store: &Store, session: &Session) -> Result<()> {
        self.apply_inner(root, store, session, true)
    }

    /// Use the same guarded restore transaction while a TUI owns stdout.
    pub fn apply_silent(self, root: &Path, store: &Store, session: &Session) -> Result<()> {
        self.apply_inner(root, store, session, false)
    }

    fn apply_inner(self, root: &Path, store: &Store, session: &Session, verbose: bool) -> Result<()> {
        let _session_lock = store.lock_idle_session(&session.id)?;
        if store.session(&session.id)?.active_branch != session.active_branch {
            bail!("session branch changed since restore preview");
        }
        let _lock = store.native_write_lock()?;
        store.ensure_no_pending_writes()?;
        // Verify again after the user has examined the preview.
        for item in &self.items {
            if snapshot(root, &item.relative)? != item.current {
                bail!("restore cancelled: {} changed since preview", item.relative);
            }
        }

        let mut applied = 0;
        for item in self.items {
            if snapshot(root, &item.relative)? != item.current {
                bail!(
                    "restore stopped after {applied} file(s): {} changed during restore",
                    item.relative
                );
            }
            let path = checked_path(root, &item.relative)?;
            match &item.desired {
                Some(content) => atomic_write(&path, content)?,
                None => fs::remove_file(&path)
                    .with_context(|| format!("deleting {}", item.relative))?,
            }
            // Record every operation, including deletions. If persistence fails,
            // surface that explicitly rather than continuing silently.
            store.add_restoration(
                session, &item.relative, item.current.as_deref(), item.desired.as_deref(),
            ).with_context(|| format!(
                "file {} changed but recording the restore failed; filesystem may be partially restored",
                item.relative
            ))?;
            applied += 1;
            if verbose { println!("[restored] {}", item.relative); }
        }
        if verbose { println!("Restore complete: {applied} tracked file(s) updated."); }
        Ok(())
    }
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().context("restore path lacks parent")?;
    let temp = parent.join(format!(".greenlight-restore-{}.tmp", Uuid::new_v4()));
    let outcome: Result<()> = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(content.as_bytes())?;
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&temp);
    }
    outcome.with_context(|| format!("writing {}", path.display()))
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
        store.add_change(&session, "step", "file.txt", Some("a\n"), "b\n").unwrap();
        let first = store.history(&session).unwrap().last().unwrap().0;
        store.add_change(&session, "step", "file.txt", Some("b\n"), "c\n").unwrap();
        fs::write(&file, "c\n").unwrap();
        let plan = RestorePlan::prepare(root.path(), &store, &session, first).unwrap();
        assert_eq!(plan.items.len(), 1);
        plan.apply(root.path(), &store, &session).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "b\n");
        assert!(RestorePlan::prepare(root.path(), &store, &session, first).unwrap().is_empty());
        assert_eq!(store.workspace_edits(&session).unwrap().len(), 3);
    }

    #[test]
    fn restoration_removes_newly_created_file() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("test", "goal", "step").unwrap();
        let start = store.history(&session).unwrap()[0].0;
        store.add_change(&session, "step", "new.txt", None, "created\n").unwrap();
        fs::write(root.path().join("new.txt"), "created\n").unwrap();
        let plan = RestorePlan::prepare(root.path(), &store, &session, start).unwrap();
        plan.apply(root.path(), &store, &session).unwrap();
        assert!(!root.path().join("new.txt").exists());
        assert!(store.workspace_edits(&session).unwrap().last().unwrap().after.is_none());
    }

    #[test]
    fn refuses_manual_changes_without_touching_files() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("test", "goal", "step").unwrap();
        let start = store.history(&session).unwrap()[0].0;
        store.add_change(&session, "step", "file.txt", Some("a\n"), "b\n").unwrap();
        fs::write(root.path().join("file.txt"), "manual\n").unwrap();
        assert!(RestorePlan::prepare(root.path(), &store, &session, start).is_err());
        assert_eq!(fs::read_to_string(root.path().join("file.txt")).unwrap(), "manual\n");
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
        let first = store.add_change(&session, "step", "a.txt", Some("old"), "one").unwrap();
        store.add_change(&session, "step", "a.txt", Some("one"), "two").unwrap();
        fs::write(root.path().join("a.txt"), "two").unwrap();
        RestorePlan::prepare(root.path(), &store, &session, first)
            .unwrap().apply(root.path(), &store, &session).unwrap();
        store.fork(&mut session, first, "step").unwrap();
        assert!(RestorePlan::prepare(root.path(), &store, &session, first).unwrap().is_empty());
        assert_eq!(fs::read_to_string(root.path().join("a.txt")).unwrap(), "one");
    }
}
