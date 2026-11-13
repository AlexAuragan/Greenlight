//! Canonical project paths and guarded migration of legacy local state.
//! State is moved as one directory so database, WAL files, blobs, approvals,
//! and sandbox cache keep their relative paths and existing history.
use anyhow::{bail, Context, Result};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

pub const PROJECT_DIR: &str = ".greenlight";
pub const LEGACY_PROJECT_DIR: &str = ".stepwright";

pub fn is_internal_state(relative: &Path) -> bool {
    let mut parts = relative.components();
    matches!(parts.next(), Some(Component::Normal(dir))
        if dir == PROJECT_DIR || dir == LEGACY_PROJECT_DIR)
        && matches!(parts.next(), Some(Component::Normal(state)) if state == "state")
}

fn existing_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => bail!("project state path is not a real directory: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("checking {}", path.display())),
    }
}

/// New projects use .greenlight/state. Existing .stepwright/state is moved
/// intact; conflicting state directories cause an explicit error, never an
/// implicit choice between two session histories.
pub fn state_directory(root: &Path) -> Result<PathBuf> {
    let current_parent = root.join(PROJECT_DIR);
    let legacy_parent = root.join(LEGACY_PROJECT_DIR);
    let current_parent_exists = existing_directory(&current_parent)?;
    // Reject legacy symlinks even when its state has not been initialized.
    let legacy_parent_exists = existing_directory(&legacy_parent)?;
    let current = current_parent.join("state");
    let legacy = legacy_parent.join("state");
    let current_exists = if current_parent_exists { existing_directory(&current)? } else { false };
    let legacy_exists = if legacy_parent_exists { existing_directory(&legacy)? } else { false };
    if current_exists && legacy_exists {
        bail!(
            "both {} and {} exist; resolve the two state directories manually before continuing",
            current.display(), legacy.display()
        );
    }
    if !current_parent_exists {
        fs::create_dir(&current_parent)
            .with_context(|| format!("creating {}", current_parent.display()))?;
    }
    if legacy_exists {
        fs::rename(&legacy, &current).with_context(|| {
            format!(
                "migrating legacy state from {} to {}; original state is retained if the move fails",
                legacy.display(), current.display()
            )
        })?;
    } else if !current_exists {
        fs::create_dir(&current)
            .with_context(|| format!("creating {}", current.display()))?;
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_projects_use_greenlight_state() {
        let root = tempfile::tempdir().unwrap();
        let path = state_directory(root.path()).unwrap();
        assert_eq!(path, root.path().join(".greenlight/state"));
        assert!(path.is_dir());
    }

    #[test]
    fn migrates_legacy_database_and_approvals_without_losing_files() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(".stepwright/state");
        fs::create_dir_all(legacy.join("blobs")).unwrap();
        fs::write(legacy.join("history.db"), "historical data").unwrap();
        fs::write(legacy.join("approvals.yaml"), "shell: []").unwrap();
        fs::write(legacy.join("blobs/123"), "artifact").unwrap();
        let destination = state_directory(root.path()).unwrap();
        assert_eq!(fs::read_to_string(destination.join("history.db")).unwrap(), "historical data");
        assert_eq!(fs::read_to_string(destination.join("approvals.yaml")).unwrap(), "shell: []");
        assert_eq!(fs::read_to_string(destination.join("blobs/123")).unwrap(), "artifact");
        assert!(!legacy.exists());
        assert_eq!(state_directory(root.path()).unwrap(), destination);
    }

    #[test]
    fn refuses_conflicting_directories_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".greenlight/state")).unwrap();
        fs::create_dir_all(root.path().join(".stepwright/state")).unwrap();
        fs::write(root.path().join(".stepwright/state/history.db"), "preserve").unwrap();
        assert!(state_directory(root.path()).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join(".stepwright/state/history.db")).unwrap(),
            "preserve"
        );
    }

    #[test]
    fn recognizes_only_exact_internal_state_prefixes() {
        assert!(is_internal_state(Path::new(".greenlight/state/history.db")));
        assert!(is_internal_state(Path::new(".stepwright/state/approvals.yaml")));
        assert!(!is_internal_state(Path::new(".greenlight/stateful.txt")));
        assert!(!is_internal_state(Path::new(".greenlight/instructions.md")));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_state_directories() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".stepwright")).unwrap();
        symlink(outside.path(), root.path().join(".stepwright/state")).unwrap();
        assert!(state_directory(root.path()).is_err());
        assert!(outside.path().is_dir());
        assert!(!root.path().join(".greenlight/state").exists());
    }
}
