//! Bounded, read-only project-file recording around workflow step boundaries.
//! Hashes detect changes; they are not copies and cannot reconstruct old files.
use crate::store::{Session, Store};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
};
use walkdir::{DirEntry, WalkDir};

const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FileDigest {
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub files: BTreeMap<String, FileDigest>,
    /// Large/non-UTF8 paths omitted from the hash set. Excluded paths are
    /// deliberately not listed (they might themselves disclose secrets).
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FileDifference {
    pub path: String,
    pub change: String,
}

fn included(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_symlink() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if matches!(
        name.as_ref(),
        ".git" | "target" | "node_modules" | ".venv" | "venv" | "__pycache__"
            | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".next"
            | ".turbo" | "dist" | "build" | ".ssh" | ".aws" | ".git-credentials"
            | "id_rsa" | "id_ed25519"
    ) || name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
    {
        return false;
    }
    if name == "state" && entry.path().parent().is_some_and(|parent| {
        parent.file_name().is_some_and(|folder| folder == ".stepwright")
    }) {
        return false;
    }
    true
}

/// Read file bytes only to calculate hashes. Never store file content.
pub fn scan(root: &Path) -> Result<Manifest> {
    let mut files = BTreeMap::new();
    let mut skipped = Vec::new();
    let mut count = 0usize;
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(included)
    {
        let entry = entry.context("walking project for workspace recording")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(root)
            .context("workspace scan path outside project root")?;
        let Some(path) = relative.to_str() else {
            skipped.push(relative.to_string_lossy().into_owned());
            continue;
        };
        count += 1;
        if count > MAX_FILES {
            bail!("workspace recording exceeds {MAX_FILES} files; narrow the scan scope");
        }
        let metadata = fs::metadata(entry.path())
            .with_context(|| format!("inspecting workspace file {path}"))?;
        if metadata.len() > MAX_FILE_BYTES {
            skipped.push(format!("{path} (larger than {MAX_FILE_BYTES} bytes)"));
            continue;
        }
        let mut file = File::open(entry.path())
            .with_context(|| format!("opening workspace file {path}"))?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 16 * 1024];
        let mut read_bytes = 0u64;
        loop {
            let n = file.read(&mut buffer)
                .with_context(|| format!("hashing workspace file {path}"))?;
            if n == 0 {
                break;
            }
            read_bytes += n as u64;
            if read_bytes > MAX_FILE_BYTES {
                bail!("workspace file {path} grew beyond scan limit during hashing");
            }
            digest.update(&buffer[..n]);
        }
        let after = fs::metadata(entry.path())
            .with_context(|| format!("rechecking workspace file {path}"))?;
        if read_bytes != metadata.len()
            || after.len() != metadata.len()
            || after.modified()? != metadata.modified()?
        {
            bail!("workspace file {path} changed during recording; retry when it is stable");
        }
        files.insert(path.to_owned(), FileDigest {
            size: read_bytes,
            sha256: format!("{:x}", digest.finalize()),
        });
    }
    Ok(Manifest { files, skipped })
}

pub fn differences(before: &Manifest, after: &Manifest) -> Vec<FileDifference> {
    let paths: BTreeSet<_> = before.files.keys().chain(after.files.keys()).collect();
    paths.into_iter().filter_map(|path| {
        let change = match (before.files.get(path), after.files.get(path)) {
            (None, Some(_)) => "created",
            (Some(_), None) => "removed",
            (Some(a), Some(b)) if a != b => "modified",
            _ => return None,
        };
        Some(FileDifference { path: path.clone(), change: change.into() })
    }).collect()
}

/// Record one immutable manifest in the branch-aware event journal.
pub fn record(
    root: &Path, store: &Store, session: &Session, step: &str, phase: &str,
) -> Result<(i64, Manifest)> {
    if !matches!(phase, "before" | "after") {
        bail!("unsupported workspace recording phase: {phase}");
    }
    let manifest = scan(root)?;
    let payload = serde_json::to_string(&manifest)?;
    let event = store.event(
        session, Some(step), &format!("workspace.manifest.{phase}"), &payload,
    )?;
    Ok((event, manifest))
}

/// Select the most recent manifest visible on the current branch (including
/// an interrupted step's 'before' manifest).
pub fn latest(store: &Store, session: &Session) -> Result<Option<(i64, String, Manifest)>> {
    for (id, step, kind, payload) in store.history(session)?.into_iter().rev() {
        if kind == "workspace.manifest.before" || kind == "workspace.manifest.after" {
            let manifest: Manifest = serde_json::from_str(&payload)
                .with_context(|| format!("invalid workspace manifest at event #{id}"))?;
            return Ok(Some((id, step, manifest)));
        }
    }
    Ok(None)
}

/// Compare the latest complete step's boundary snapshots. This reports
/// observed changes, without claiming whether a user or tool caused them.
pub fn latest_step_changes(
    store: &Store, session: &Session,
) -> Result<Option<(String, Vec<FileDifference>)>> {
    let history = store.history(session)?;
    let Some(after_index) = history.iter()
        .rposition(|(_, _, kind, _)| kind == "workspace.manifest.after") else {
        return Ok(None);
    };
    let (_, step, _, after_payload) = &history[after_index];
    let before = history[..after_index].iter().rev()
        .find(|(_, previous_step, kind, _)| {
            previous_step == step && kind == "workspace.manifest.before"
        })
        .with_context(|| format!("missing workspace baseline for step {step}"))?;
    let prior: Manifest = serde_json::from_str(&before.3)
        .context("invalid prior workspace manifest")?;
    let after: Manifest = serde_json::from_str(after_payload)
        .context("invalid completed workspace manifest")?;
    Ok(Some((step.clone(), differences(&prior, &after))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scan_records_creations_edits_and_deletions() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.rs"), "first").unwrap();
        fs::write(root.path().join("deleted.txt"), "old").unwrap();
        let before = scan(root.path()).unwrap();

        fs::write(root.path().join("src/main.rs"), "second").unwrap();
        fs::write(root.path().join("created.txt"), "new").unwrap();
        fs::remove_file(root.path().join("deleted.txt")).unwrap();
        let after = scan(root.path()).unwrap();
        assert_eq!(differences(&before, &after), vec![
            FileDifference { path: "created.txt".into(), change: "created".into() },
            FileDifference { path: "deleted.txt".into(), change: "removed".into() },
            FileDifference { path: "src/main.rs".into(), change: "modified".into() },
        ]);
    }

    #[test]
    fn scan_excludes_caches_and_sensitive_paths() {
        let root = tempdir().unwrap();
        for dir in [".git", "target", ".stepwright/state", "node_modules"] {
            fs::create_dir_all(root.path().join(dir)).unwrap();
            fs::write(root.path().join(dir).join("private.txt"), "secret").unwrap();
        }
        fs::write(root.path().join(".env"), "TOKEN=secret").unwrap();
        fs::write(root.path().join(".env.local"), "TOKEN=secret").unwrap();
        fs::write(root.path().join("main.rs"), "visible").unwrap();
        let manifest = scan(root.path()).unwrap();
        assert_eq!(
            manifest.files.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["main.rs"],
        );
    }

    #[test]
    fn manifest_round_trips_from_branch_history() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("wf", "goal", "implement").unwrap();
        fs::write(root.path().join("main.rs"), "old").unwrap();
        let (id, first) = record(root.path(), &store, &session, "implement", "before").unwrap();
        let (found, _, parsed) = latest(&store, &session).unwrap().unwrap();
        assert_eq!(found, id);
        assert_eq!(parsed.files, first.files);
        store.fork(&mut session, id, "implement").unwrap();
        assert_eq!(latest(&store, &session).unwrap().unwrap().0, id);
    }

    #[test]
    fn step_change_history_uses_matching_step_boundaries() {
        let root = tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("wf", "goal", "implement").unwrap();
        fs::write(root.path().join("main.rs"), "original").unwrap();
        record(root.path(), &store, &session, "implement", "before").unwrap();
        fs::write(root.path().join("main.rs"), "updated").unwrap();
        record(root.path(), &store, &session, "implement", "after").unwrap();
        let (step, changes) = latest_step_changes(&store, &session).unwrap().unwrap();
        assert_eq!(step, "implement");
        assert_eq!(changes, vec![
            FileDifference { path: "main.rs".into(), change: "modified".into() },
        ]);
        record(root.path(), &store, &session, "verify", "before").unwrap();
        let (_, changes) = latest_step_changes(&store, &session).unwrap().unwrap();
        assert_eq!(changes.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn never_traverses_symlinked_directories() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let external = tempdir().unwrap();
        fs::write(external.path().join("secret.txt"), "secret").unwrap();
        symlink(external.path(), root.path().join("linked")).unwrap();
        assert!(scan(root.path()).unwrap().files.is_empty());
    }
}
