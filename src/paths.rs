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

/// User-owned host settings/credentials must not be accessible to agent tools.
pub fn is_private_path(relative: &Path) -> bool {
    is_internal_state(relative)
        || matches!(
            relative.to_str(),
            Some("greenlight.yaml" | "stepwright.yaml" | ".greenlight/config.yaml")
        )
        || relative.components().any(|part| {
            let name = part.as_os_str().to_string_lossy();
            name.starts_with(".env") || matches!(name.as_ref(), ".git" | ".ssh")
        })
}

/// Read model-visible project files under the same privacy boundary as tools.
pub(crate) fn read_project_file(root: &Path, path: &Path) -> Result<String> {
    let directory = crate::anchored_fs::Dir::open(root)?;
    let root = root.canonicalize()?;
    if let Ok(relative) = path.strip_prefix(&root) {
        if is_private_path(relative) {
            bail!("private project file: {}", path.display());
        }
    }
    let resolved = path.canonicalize()?;
    let relative = resolved
        .strip_prefix(&root)
        .context("file escapes project root")?;
    if is_private_path(relative) {
        bail!("private project file: {}", path.display());
    }
    if !resolved.is_file() {
        bail!("project file is not a regular file: {}", path.display());
    }
    directory.read_string(relative, u64::MAX)
}

pub fn config_path(root: &Path) -> Result<PathBuf> {
    existing_directory(&root.join(PROJECT_DIR))?;
    for relative in [
        ".greenlight/config.yaml",
        "greenlight.yaml",
        "stepwright.yaml",
    ] {
        let path = root.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    bail!("No Greenlight configuration in {}. Run `greenlight init` to create .greenlight/config.yaml (legacy greenlight.yaml/stepwright.yaml also supported).",root.display())
}

pub fn workflows_directory(root: &Path) -> Result<PathBuf> {
    existing_directory(&root.join(PROJECT_DIR))?;
    let local = root.join(PROJECT_DIR).join("workflows");
    if existing_directory(&local)? {
        Ok(local)
    } else {
        Ok(root.join("workflows"))
    }
}

pub fn is_internal_state(relative: &Path) -> bool {
    let mut parts = relative.components();
    matches!(parts.next(), Some(Component::Normal(dir))
        if dir == PROJECT_DIR || dir == LEGACY_PROJECT_DIR)
        && matches!(parts.next(), Some(Component::Normal(state)) if state == "state")
}

pub(crate) fn existing_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => bail!(
            "project state path is not a real directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("checking {}", path.display())),
    }
}

/// New projects use .greenlight/state. Existing .stepwright/state is moved
/// intact; conflicting state directories cause an explicit error, never an
/// implicit choice between two session histories.
pub fn state_directory(root: &Path) -> Result<PathBuf> {
    Ok(state_directory_handle(root)?.0)
}
pub(crate) fn state_directory_handle(root: &Path) -> Result<(PathBuf, crate::anchored_fs::Dir)> {
    let directory = crate::anchored_fs::Dir::open(root)?;
    let exists = |path: &str| -> Result<bool> {
        match directory.open_dir(Path::new(path)) {
            Ok(_) => Ok(true),
            Err(error)
                if error
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    };
    // Check both parents before mutation, including legacy symlinks without state.
    exists(PROJECT_DIR)?;
    exists(LEGACY_PROJECT_DIR)?;
    let current = exists(".greenlight/state")?;
    let legacy = exists(".stepwright/state")?;
    if current && legacy {
        bail!("both state directories exist; resolve manually before continuing");
    }
    directory.create_dirs(Path::new(PROJECT_DIR))?;
    if legacy {
        directory
            .target(Path::new(".stepwright/state"))?
            .rename_to(&directory.target(Path::new(".greenlight/state"))?)?;
    }
    let state = directory.create_dirs(Path::new(".greenlight/state"))?;
    Ok((root.join(".greenlight/state"), state))
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
        assert_eq!(
            fs::read_to_string(destination.join("history.db")).unwrap(),
            "historical data"
        );
        assert_eq!(
            fs::read_to_string(destination.join("approvals.yaml")).unwrap(),
            "shell: []"
        );
        assert_eq!(
            fs::read_to_string(destination.join("blobs/123")).unwrap(),
            "artifact"
        );
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
        assert!(is_internal_state(Path::new(
            ".stepwright/state/approvals.yaml"
        )));
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
