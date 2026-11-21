//! Persistent, human-created permission grants. Project grants are private to
//! the local state directory; global grants live in the user's config directory.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::fs;
use std::{
    collections::BTreeSet,
    env, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Grants {
    #[serde(default)]
    shell: BTreeSet<String>,
    #[serde(default)]
    write: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Scope {
    Project,
    Global,
}

pub struct ApprovalStore {
    project_path: PathBuf,
    global_path: Option<PathBuf>,
    project: Grants,
    global: Grants,
}

impl ApprovalStore {
    #[cfg(test)]
    pub(crate) fn isolated(root: &Path) -> Result<Self> {
        Self::from_paths(
            root.join("project-grants.yaml"),
            Some(root.join("global-grants.yaml")),
        )
    }
    pub fn open(root: &Path) -> Result<Self> {
        let project_path = crate::paths::state_directory(root)?.join("approvals.yaml");
        let global_path = Self::global_config_path();
        Self::from_paths(project_path, global_path)
    }

    fn global_config_path() -> Option<PathBuf> {
        if let Some(value) = env::var_os("XDG_CONFIG_HOME") {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Some(path.join("greenlight/approvals.yaml"));
            }
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".config/greenlight/approvals.yaml"))
    }

    fn from_paths(project_path: PathBuf, global_path: Option<PathBuf>) -> Result<Self> {
        let project = Self::read(&project_path)?;
        let global = match &global_path {
            Some(path) => Self::read(path)?,
            None => Grants::default(),
        };
        Ok(Self {
            project_path,
            global_path,
            project,
            global,
        })
    }

    fn read(path: &Path) -> Result<Grants> {
        let parent = path.parent().context("invalid approval path")?;
        let directory = match crate::anchored_fs::Dir::open(parent) {
            Ok(dir) => dir,
            Err(error)
                if error
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
            {
                return Ok(Grants::default())
            }
            Err(error) => return Err(error),
        };
        Self::read_at(
            &directory,
            path.file_name().context("approval path lacks filename")?,
        )
    }
    fn read_at(directory: &crate::anchored_fs::Dir, name: &std::ffi::OsStr) -> Result<Grants> {
        let text = match directory.read_string(
            Path::new(name),
            crate::resource_limits::Limits::default().source_bytes as u64,
        ) {
            Ok(text) => text,
            Err(error)
                if error
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
            {
                return Ok(Grants::default())
            }
            Err(error) => return Err(error),
        };
        serde_yaml::from_str(&text).context("invalid approvals file")
    }

    /// An allowed command is only the *exact* original shell string.
    /// No prefix, wildcard, or command-name matching: shell metacharacters
    /// otherwise turn seemingly harmless allowlists into arbitrary execution.
    pub fn matched_scope(&self, kind: &str, detail: &str) -> Option<Scope> {
        let matches = |grants: &Grants| match kind {
            "shell" => grants.shell.contains(detail),
            "write" => grants.write,
            _ => false,
        };
        if matches(&self.project) {
            Some(Scope::Project)
        } else if matches(&self.global) {
            Some(Scope::Global)
        } else {
            None
        }
    }

    pub fn grant(&mut self, scope: Scope, kind: &str, detail: &str) -> Result<()> {
        let path = match scope {
            Scope::Project => &self.project_path,
            Scope::Global => self
                .global_path
                .as_ref()
                .context("global approvals unavailable: set HOME or an absolute XDG_CONFIG_HOME")?,
        };
        // Lock a stable sibling, not the replaced YAML inode. Only the selected
        // scope is locked, so different projects/scopes cannot deadlock each other.
        let parent = path.parent().context("invalid approvals path")?;
        let directory = crate::anchored_fs::Dir::open(Path::new("/"))?
            .create_dirs(parent.strip_prefix("/")?)?;
        let _lock = crate::locks::LockGuard::wait(
            directory.lock_file(Path::new(
                path.with_extension("lock")
                    .file_name()
                    .context("approval lock lacks name")?,
            ))?,
        )
        .context("locking approval store")?;
        let name = path.file_name().context("approval file lacks name")?;
        let mut updated = Self::read_at(&directory, name)?;
        match kind {
            "shell" => {
                if detail.is_empty() {
                    bail!("cannot save an empty shell command");
                }
                updated.shell.insert(detail.to_string());
            }
            "write" => updated.write = true,
            _ => bail!("persistent approval is supported only for shell and write"),
        };
        directory
            .target(Path::new(name))?
            .atomic_write(serde_yaml::to_string(&updated)?.as_bytes())?;
        match scope {
            Scope::Project => self.project = updated,
            Scope::Global => self.global = updated,
        }
        Ok(())
    }

    pub fn describe(&self) -> Result<String> {
        let project = serde_yaml::to_string(&self.project)?;
        let global = match &self.global_path {
            Some(path) => format!(
                "Global ({}):\n{}",
                path.display(),
                serde_yaml::to_string(&self.global)?
            ),
            None => "Global: unavailable (set HOME or XDG_CONFIG_HOME)\n".into(),
        };
        Ok(format!(
            "Project ({}):\n{}\n{}",
            self.project_path.display(),
            project,
            global
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approvals_persist_and_are_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("project/approvals.yaml");
        let global = dir.path().join("global/approvals.yaml");

        let mut grants = ApprovalStore::from_paths(local.clone(), Some(global.clone())).unwrap();
        assert!(grants.matched_scope("shell", "ls -la").is_none());
        grants.grant(Scope::Project, "shell", "ls -la").unwrap();
        grants.grant(Scope::Global, "shell", "pwd").unwrap();
        grants.grant(Scope::Project, "write", "unused").unwrap();

        let again = ApprovalStore::from_paths(local, Some(global)).unwrap();
        assert!(matches!(
            again.matched_scope("shell", "ls -la"),
            Some(Scope::Project)
        ));
        assert!(matches!(
            again.matched_scope("shell", "pwd"),
            Some(Scope::Global)
        ));
        assert!(matches!(
            again.matched_scope("write", "another/file.rs"),
            Some(Scope::Project)
        ));
        assert!(again
            .matched_scope("shell", "ls -la; rm important")
            .is_none());
        assert!(again.matched_scope("shell", "ls -l").is_none());
    }

    #[test]
    fn invalid_file_is_not_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("approvals.yaml");
        fs::write(&local, "shell: not-a-list\n").unwrap();
        assert!(ApprovalStore::from_paths(local, None).is_err());
    }

    #[test]
    fn missing_home_disables_global_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut approvals = ApprovalStore::from_paths(dir.path().join("local.yaml"), None).unwrap();
        approvals.grant(Scope::Project, "shell", "ls").unwrap();
        assert!(approvals.grant(Scope::Global, "shell", "ls").is_err());
    }

    #[test]
    fn grant_preserves_other_process_entries() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("local.yaml");
        let mut a = ApprovalStore::from_paths(local.clone(), None).unwrap();
        let mut b = ApprovalStore::from_paths(local.clone(), None).unwrap();
        a.grant(Scope::Project, "shell", "ls").unwrap();
        b.grant(Scope::Project, "shell", "pwd").unwrap();
        let latest = ApprovalStore::from_paths(local, None).unwrap();
        assert!(latest.matched_scope("shell", "ls").is_some());
        assert!(latest.matched_scope("shell", "pwd").is_some());
    }
    #[test]
    fn concurrent_process_grants_merge_under_the_selected_scope_lock() {
        const ROOT: &str = "GREENLIGHT_TEST_GRANT_ROOT";
        const INDEX: &str = "GREENLIGHT_TEST_GRANT_INDEX";
        const GLOBAL: &str = "GREENLIGHT_TEST_GRANT_GLOBAL";
        if let Some(root) = env::var_os(ROOT) {
            let root = PathBuf::from(root);
            let index = env::var(INDEX).unwrap();
            let scope = if env::var_os(GLOBAL).is_some() {
                Scope::Global
            } else {
                Scope::Project
            };
            let mut store = ApprovalStore::from_paths(
                root.join("project/approvals.yaml"),
                Some(root.join("global/approvals.yaml")),
            )
            .unwrap();
            fs::write(root.join(format!("ready-{index}")), "ready").unwrap();
            store
                .grant(scope, "shell", &format!("fixture-command-{index}"))
                .unwrap();
            return;
        }
        for scope in [Scope::Project, Scope::Global] {
            let root = tempfile::tempdir().unwrap();
            let local = root.path().join("project/approvals.yaml");
            let global = root.path().join("global/approvals.yaml");
            let path = match scope {
                Scope::Project => &local,
                Scope::Global => &global,
            };
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let lock = crate::locks::LockGuard::wait(
                fs::File::create(path.with_extension("lock")).unwrap(),
            )
            .unwrap();
            let mut children = Vec::new();
            for index in 0..6 {
                let mut command = std::process::Command::new(env::current_exe().unwrap());
                command.args(["--exact", "approvals::tests::concurrent_process_grants_merge_under_the_selected_scope_lock"])
                    .env(ROOT, root.path()).env(INDEX, index.to_string())
                    .env_remove(GLOBAL).stdout(std::process::Stdio::null());
                if matches!(scope, Scope::Global) {
                    command.env(GLOBAL, "1");
                }
                children.push(command.spawn().unwrap());
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !(0..6).all(|index| root.path().join(format!("ready-{index}")).exists()) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "grant children did not start"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(!path.exists());
            assert!(children
                .iter_mut()
                .all(|child| child.try_wait().unwrap().is_none()));
            drop(lock);
            for mut child in children {
                assert!(child.wait().unwrap().success());
            }
            let store = ApprovalStore::from_paths(local, Some(global)).unwrap();
            for index in 0..6 {
                assert!(matches!(
                    (
                        scope,
                        store.matched_scope("shell", &format!("fixture-command-{index}"))
                    ),
                    (Scope::Project, Some(Scope::Project)) | (Scope::Global, Some(Scope::Global))
                ));
            }
        }
    }
}
