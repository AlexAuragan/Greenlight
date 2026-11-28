//! User-requested observations and native-journal patches. Never runs Git,
//! changes its index, captures external bodies, or applies an exported patch.
use crate::{
    anchored_fs::Dir,
    store::{Session, Store},
    workspace,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    path::{Component, Path, PathBuf},
};
const MAX_PATCH: usize = 16 * 1024 * 1024;
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn optional(dir: &Dir, path: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    match dir.read(Path::new(path), limit) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitIdentity {
    pub kind: String,
    pub head: Option<String>,
    pub commit: Option<String>,
    pub index_sha256: Option<String>,
}
fn oid(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}
fn identity(root: &Path) -> Result<GitIdentity> {
    let directory = Dir::open(root)?;
    let mut result = GitIdentity {
        kind: "no_project_root_git_directory".into(),
        head: None,
        commit: None,
        index_sha256: None,
    };
    match directory.metadata(Path::new(".git")) {
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(result)
        }
        Err(error) => return Err(error),
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!("Checkpoint refuses a symlink .git directory")
        }
        Ok(meta) if meta.is_file() => {
            result.kind = "gitfile_unsupported_no_external_metadata_read".into();
            return Ok(result);
        }
        Ok(meta) if !meta.is_dir() => bail!("Unsupported .git entry"),
        _ => {}
    }
    let git = directory.open_dir(Path::new(".git"))?;
    let head = git.read_string(Path::new("HEAD"), 4096)?;
    let head = head.trim_end_matches('\n');
    if let Some(reference) = head.strip_prefix("ref: ") {
        if !reference.starts_with("refs/")
            || reference
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            || Path::new(reference)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
        {
            bail!("Unsupported Git HEAD reference");
        }
        if let Some(bytes) = optional(&git, reference, 4096)? {
            let value = String::from_utf8(bytes)?;
            let value = value.trim();
            if !oid(value) {
                bail!("Malformed Git reference identity");
            }
            result.commit = Some(value.into());
        } else if let Some(bytes) = optional(&git, "packed-refs", 4 * 1024 * 1024)? {
            for line in std::str::from_utf8(&bytes)?.lines() {
                if let Some((value, name)) = line.split_once(' ') {
                    if name == reference {
                        if !oid(value) {
                            bail!("Malformed packed Git reference identity");
                        }
                        result.commit = Some(value.into());
                        break;
                    }
                }
            }
        }
    } else if oid(head) {
        result.commit = Some(head.into());
    } else {
        bail!("Malformed Git HEAD identity");
    }
    result.kind = "project_root_git_directory".into();
    result.head = Some(head.into());
    result.index_sha256 = optional(&git, "index", 16 * 1024 * 1024)?.map(|bytes| hash(&bytes));
    Ok(result)
}
#[derive(Serialize, Deserialize)]
pub struct Checkpoint {
    pub schema: String,
    pub label: String,
    pub session: String,
    pub branch: String,
    pub history_cutoff: Option<i64>,
    pub git: GitIdentity,
    pub workspace: workspace::Manifest,
}
impl Store {
    pub fn record_git_checkpoint(
        &self,
        root: &Path,
        selected: &Session,
        label: &str,
    ) -> Result<i64> {
        if label.len() > 256 || label.chars().any(char::is_control) {
            bail!("Checkpoint label must be at most 256 bytes without control characters");
        }
        self.with_project_control(selected, "checkpoint", |_| {
            self.ensure_no_pending_writes()?;
            let _lock = self.native_write_lock()?;
            let git = identity(root)?;
            let workspace = workspace::scan(root)?;
            if git != identity(root)? { bail!("Git HEAD/index changed during checkpoint; retry after inspection"); }
            let checkpoint = Checkpoint { schema: "greenlight.git-checkpoint.v1".into(), label: label.into(), session: selected.id.clone(), branch: selected.active_branch.clone(), history_cutoff: self.history(selected)?.last().map(|e| e.0), git, workspace };
            let blob = crate::conversation_dump::save_message(self, &serde_json::to_value(&checkpoint)?)?;
            self.event(selected, None, "workspace.checkpoint", &serde_json::json!({"label":label,"blob":blob,"scope":"Observation only; no Git commit/index update or external body capture"}).to_string())
        })
    }
    pub fn git_checkpoint(&self, selected: &Session, event: i64) -> Result<Checkpoint> {
        let history = self.history(selected)?;
        let row = history
            .iter()
            .find(|e| e.0 == event && e.2 == "workspace.checkpoint")
            .context("Checkpoint not visible on this branch")?;
        let reference: serde_json::Value = serde_json::from_str(&row.3)?;
        let blob: String = serde_json::from_value(reference["blob"].clone())?;
        let cp: Checkpoint =
            serde_json::from_value(crate::conversation_dump::load_message(self, &blob)?)?;
        if cp.schema != "greenlight.git-checkpoint.v1"
            || cp.session != selected.id
            || cp.branch != selected.active_branch
        {
            bail!("Checkpoint identity/branch mismatch");
        }
        Ok(cp)
    }
    pub fn native_patch(&self, selected: &Session, checkpoint: Option<i64>) -> Result<Vec<u8>> {
        let tx = self.db.unchecked_transaction()?;
        if self.session(&selected.id)?.active_branch != selected.active_branch {
            bail!("Branch changed; refresh before export");
        }
        let history = self.history(selected)?;
        let cp = checkpoint
            .map(|event| self.git_checkpoint(selected, event))
            .transpose()?;
        if cp.as_ref().is_some_and(|c| !c.workspace.skipped.is_empty()) {
            bail!("Checkpoint scan was incomplete; cannot establish a complete patch baseline");
        }
        let mut files: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
        let max_patch = MAX_PATCH.min(self.limits.export_bytes);
        let mut budget = crate::resource_limits::Budget::new(max_patch);
        for edit in self.workspace_edits(selected)? {
            if checkpoint.is_some_and(|event| edit.event_id <= event) {
                continue;
            }
            let path = Path::new(&edit.path);
            if path.as_os_str().is_empty()
                || path.is_absolute()
                || path
                    .components()
                    .any(|p| !matches!(p, Component::Normal(_)))
                || crate::paths::is_private_path(path)
            {
                bail!("Unsupported/private patch path");
            }
            if edit.before.as_ref().is_some_and(|s| s.contains('\0'))
                || edit.after.as_ref().is_some_and(|s| s.contains('\0'))
            {
                bail!("Binary native content cannot be exported as a text patch");
            }
            budget.charge(
                edit.path.len()
                    + edit.before.as_ref().map_or(0, String::len)
                    + edit.after.as_ref().map_or(0, String::len),
            )?;
            if let Some((_, after)) = files.get_mut(&edit.path) {
                if *after != edit.before {
                    bail!(
                        "Native history gap in {}; shell/manual changes cannot be inferred",
                        edit.path
                    );
                }
                *after = edit.after;
            } else {
                if let Some(cp) = &cp {
                    if !workspace::path_in_scope(path) {
                        bail!("Path outside checkpoint manifest scope: {}", edit.path);
                    }
                    let before = edit.before.as_ref().map(|s| workspace::FileDigest {
                        size: s.len() as u64,
                        sha256: hash(s.as_bytes()),
                    });
                    if cp.workspace.files.get(&edit.path) != before.as_ref() {
                        bail!("First native preimage differs from checkpoint for {}; external edits are not a native patch", edit.path);
                    }
                }
                files.insert(edit.path, (edit.before, edit.after));
            }
        }
        let mut patch = format!("# Greenlight native journal patch; shell/manual changes and file modes are outside its coverage.\n# {}\n", serde_json::json!({"session":selected.id,"branch":selected.active_branch,"checkpoint":checkpoint,"cutoff":history.last().map(|e|e.0),"git":cp.as_ref().map(|c| &c.git)}));
        let mut changed = 0;
        for (path, (before, after)) in files {
            if before == after {
                continue;
            }
            changed += 1;
            let a = quote(&format!("a/{path}"));
            let b = quote(&format!("b/{path}"));
            patch.push_str(&format!("diff --git {a} {b}\n"));
            if before.is_none() {
                patch.push_str("new file mode 100644\n");
            }
            if after.is_none() {
                patch.push_str("deleted file mode 100644\n");
            }
            let diff = crate::changes::unified(
                "native",
                before.as_deref(),
                after.as_deref().unwrap_or(""),
            );
            let body = diff.splitn(3, '\n').nth(2).unwrap_or("");
            // Extended mode headers represent empty-file creation/deletion too.
            if !body.is_empty() {
                patch.push_str(&format!(
                    "--- {}\n+++ {}\n{body}",
                    if before.is_none() { "/dev/null" } else { &a },
                    if after.is_none() { "/dev/null" } else { &b }
                ));
            }
            if patch.len() > max_patch {
                bail!("Native patch exceeds the 16 MiB/configured export budget; no complete export produced");
            }
        }
        if changed == 0 {
            bail!("No net native edits in the selected history range");
        }
        tx.commit()?;
        Ok(patch.into_bytes())
    }
    pub fn save_native_patch(
        &self,
        selected: &Session,
        checkpoint: Option<i64>,
    ) -> Result<PathBuf> {
        let bytes = self.native_patch(selected, checkpoint)?;
        let parent = self.directory.parent().context("Missing state parent")?;
        let exports = Dir::open(parent)?.create_dirs(Path::new("exports"))?;
        let name = format!("native-{}.patch", uuid::Uuid::new_v4());
        exports.target(Path::new(&name))?.publish_new(&bytes)?;
        Ok(parent.join("exports").join(name))
    }
}
fn quote(path: &str) -> String {
    let mut out = String::from("\"");
    for byte in path.bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b' '..=b'~' => out.push(byte as char),
            _ => out.push_str(&format!("\\{byte:03o}")),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};
    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    #[test]
    fn exported_patch_applies_creations_deletions_quoted_paths_and_missing_newlines() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let store = Store::open(source.path()).unwrap();
        let s = store.create("demo", "goal", "step").unwrap();
        let cases = [
            ("edit.txt", Some("before"), Some("after\n")),
            ("delete.txt", Some("delete\n"), None),
            ("create.txt", None, Some("created")),
            ("empty.txt", None, Some("")),
            ("empty-delete.txt", Some(""), None),
            ("space \" tab\t café.txt", Some("a\n"), Some("b\n")),
        ];
        for (path, before, after) in cases {
            if let Some(before) = before {
                fs::write(target.path().join(path), before).unwrap();
            }
            store.add_restoration(&s, path, before, after).unwrap();
        }
        let patch = store.save_native_patch(&s, None).unwrap();
        git(
            target.path(),
            &["apply", "--check", patch.to_str().unwrap()],
        );
        git(target.path(), &["apply", patch.to_str().unwrap()]);
        for (path, _, after) in cases {
            assert_eq!(
                fs::read_to_string(target.path().join(path)).ok().as_deref(),
                after
            );
        }
        assert_eq!(
            store
                .history(&s)
                .unwrap()
                .iter()
                .filter(|e| e.2 == "workspace.restored")
                .count(),
            cases.len()
        );
    }
    #[test]
    fn checkpoint_observes_git_without_reading_config_or_changing_index() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        fs::write(root.path().join("staged.txt"), "staged").unwrap();
        git(root.path(), &["add", "--", "staged.txt"]);
        fs::write(root.path().join("staged.txt"), "manual unstaged").unwrap();
        let before = fs::read(root.path().join(".git/index")).unwrap();
        // Deliberately invalid config must not even be parsed by checkpointing.
        fs::write(root.path().join(".git/config"), "not valid git config").unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "step").unwrap();
        let event = store
            .record_git_checkpoint(root.path(), &s, "baseline")
            .unwrap();
        let cp = store.git_checkpoint(&s, event).unwrap();
        assert_eq!(cp.git.index_sha256, Some(hash(&before)));
        assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(root.path().join("staged.txt")).unwrap(),
            "manual unstaged"
        );
        assert!(!cp.workspace.files.contains_key(".git/config"));
        store
            .add_change(&s, "step", "staged.txt", Some("manual unstaged"), "native")
            .unwrap();
        let patch = store.native_patch(&s, Some(event)).unwrap();
        assert!(String::from_utf8(patch)
            .unwrap()
            .contains("-manual unstaged"));
        assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), before);
    }
    #[test]
    fn gaps_manual_preimages_and_foreign_checkpoints_refuse_export() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "step").unwrap();
        fs::write(root.path().join("a"), "baseline").unwrap();
        let cp = store
            .record_git_checkpoint(root.path(), &s, "base")
            .unwrap();
        store
            .add_change(&s, "step", "a", Some("manual"), "native")
            .unwrap();
        assert!(store
            .native_patch(&s, Some(cp))
            .unwrap_err()
            .to_string()
            .contains("preimage differs"));
        store
            .add_change(&s, "step", "a", Some("external gap"), "later")
            .unwrap();
        assert!(store
            .native_patch(&s, None)
            .unwrap_err()
            .to_string()
            .contains("history gap"));
        let other = store.create("demo", "other", "step").unwrap();
        assert!(store.git_checkpoint(&other, cp).is_err());
        assert!(store.native_patch(&other, Some(cp)).is_err());
    }
    #[test]
    fn gitfiles_are_not_followed_and_symlink_metadata_is_refused() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".git"), "gitdir: /outside/private").unwrap();
        assert!(identity(root.path())
            .unwrap()
            .kind
            .starts_with("gitfile_unsupported"));
        fs::remove_file(root.path().join(".git")).unwrap();
        std::os::unix::fs::symlink("/outside/private", root.path().join(".git")).unwrap();
        assert!(identity(root.path()).is_err());
    }
    #[test]
    fn packed_detached_and_symlinked_refs_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let commit = "0123456789012345678901234567890123456789";
        fs::write(
            root.path().join(".git/packed-refs"),
            format!("{commit} refs/heads/main\n"),
        )
        .unwrap();
        assert_eq!(
            identity(root.path()).unwrap().commit.as_deref(),
            Some(commit)
        );
        fs::write(root.path().join(".git/HEAD"), format!("{commit}\n")).unwrap();
        assert_eq!(
            identity(root.path()).unwrap().commit.as_deref(),
            Some(commit)
        );
        fs::remove_file(root.path().join(".git/HEAD")).unwrap();
        std::os::unix::fs::symlink("/outside/private", root.path().join(".git/HEAD")).unwrap();
        assert!(identity(root.path()).is_err());
    }
    #[test]
    fn incomplete_baselines_and_small_export_budgets_produce_no_patch() {
        let root = tempfile::tempdir().unwrap();
        fs::File::create(root.path().join("large"))
            .unwrap()
            .set_len(20 * 1024 * 1024 + 1)
            .unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "step").unwrap();
        let cp = store
            .record_git_checkpoint(root.path(), &s, "incomplete")
            .unwrap();
        store.add_change(&s, "step", "a", None, "created").unwrap();
        assert!(store
            .native_patch(&s, Some(cp))
            .unwrap_err()
            .to_string()
            .contains("incomplete"));
        store.limits.export_bytes = 1;
        assert!(store.save_native_patch(&s, None).is_err());
        assert!(!store.directory.parent().unwrap().join("exports").exists());
    }
    #[test]
    fn unsafe_and_binary_journal_records_never_publish_a_patch() {
        for (path, content) in [
            ("../outside", "text"),
            (".git/config", "text"),
            ("", "text"),
            ("binary", "nul\0body"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let s = store.create("demo", "goal", "step").unwrap();
            store.add_change(&s, "step", path, None, content).unwrap();
            assert!(store.save_native_patch(&s, None).is_err());
            assert!(!store.directory.parent().unwrap().join("exports").exists());
        }
    }
}
