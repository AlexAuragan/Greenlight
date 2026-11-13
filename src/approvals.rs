//! Persistent, human-created permission grants. Project grants are private to
//! the local state directory; global grants live in the user's config directory.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

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
        Ok(Self { project_path, global_path, project, global })
    }

    fn read(path: &Path) -> Result<Grants> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(Grants::default());
            }
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        serde_yaml::from_str(&text)
            .with_context(|| format!("invalid approvals file {}", path.display()))
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
            Scope::Global => self.global_path.as_ref().context(
                "global approvals unavailable: set HOME or an absolute XDG_CONFIG_HOME",
            )?,
        };
        // Reload before editing so independent processes do not normally drop
        // each other's grants. Concurrent writes still need locking eventually.
        let mut updated = Self::read(path)?;
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
        Self::save(path, &updated)?;
        match scope {
            Scope::Project => self.project = updated,
            Scope::Global => self.global = updated,
        }
        Ok(())
    }

    fn save(path: &Path, grants: &Grants) -> Result<()> {
        let parent = path.parent().context("invalid approvals path")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("creating approval directory {}", parent.display()))?;
        let temporary = parent.join(format!(".approvals-{}.tmp", Uuid::new_v4()));
        let text = serde_yaml::to_string(grants)?;
        let result: Result<()> = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.with_context(|| format!("saving approvals to {}", path.display()))
    }

    pub fn describe(&self) -> Result<String> {
        let project = serde_yaml::to_string(&self.project)?;
        let global = match &self.global_path {
            Some(path) => format!("Global ({}):\n{}", path.display(), serde_yaml::to_string(&self.global)?),
            None => "Global: unavailable (set HOME or XDG_CONFIG_HOME)\n".into(),
        };
        Ok(format!("Project ({}):\n{}\n{}", self.project_path.display(), project, global))
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
        assert!(matches!(again.matched_scope("shell", "ls -la"), Some(Scope::Project)));
        assert!(matches!(again.matched_scope("shell", "pwd"), Some(Scope::Global)));
        assert!(matches!(again.matched_scope("write", "another/file.rs"), Some(Scope::Project)));
        assert!(again.matched_scope("shell", "ls -la; rm important").is_none());
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
}
