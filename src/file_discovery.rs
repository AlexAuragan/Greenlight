//! Bounded directory reads. No shell, file bodies, symlink traversal or hashing.
use anyhow::{bail, Result};
use serde::Serialize;
use std::path::{Component, Path};

const MAX_VISITED: usize = 10_000;
const MAX_ENTRIES: usize = 500;
const MAX_BYTES: usize = 24_000;
const MAX_DEPTH: usize = 16;
#[derive(Serialize)]
struct Entry {
    path: String,
    kind: &'static str,
}
#[derive(Serialize)]
struct Listing<'a> {
    directory: String,
    filter: Option<&'a str>,
    scope: &'static str,
    entries: Vec<Entry>,
    truncated: bool,
    unreadable_entries: usize,
    limits: Limits,
}
#[derive(Serialize)]
struct Limits {
    entries: usize,
    visited: usize,
    bytes: usize,
    depth: usize,
}
fn included(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return true;
    }
    crate::workspace::path_in_scope(relative) && !crate::paths::is_private_path(relative)
        && !relative.components().any(|c|matches!(c,Component::Normal(name) if name==crate::paths::PROJECT_DIR || name==crate::paths::LEGACY_PROJECT_DIR))
}
#[cfg(test)]
pub fn list(root: &Path, directory: &Path, filter: Option<&str>) -> Result<String> {
    let root = root.canonicalize()?;
    let directory = directory.canonicalize()?;
    list_anchored(
        &crate::anchored_fs::Dir::open(&root)?,
        directory.strip_prefix(&root)?,
        filter,
    )
}
pub fn list_anchored(
    root: &crate::anchored_fs::Dir,
    directory: &Path,
    filter: Option<&str>,
) -> Result<String> {
    if !included(Path::new(""), directory) {
        bail!("directory is outside discovery scope");
    }
    root.open_dir(directory)?;
    if filter.is_some_and(|f| f.len() > 256 || f.chars().any(char::is_control)) {
        bail!("directory input filter must be at most 256 UTF-8 bytes without control characters");
    }
    let mut entries = Vec::new();
    let mut visited = 1;
    let mut bytes = 0;
    let mut truncated = false;
    let mut unreadable = 0;
    let mut pending = vec![(directory.to_path_buf(), 0usize)];
    'directories: while let Some((relative, depth)) = pending.pop() {
        let items = match root.open_dir(&relative).and_then(|dir| dir.entries()) {
            Ok(items) => items,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        for item in items {
            let item = match item {
                Ok(item) => item,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            if visited >= MAX_VISITED {
                truncated = true;
                break 'directories;
            }
            visited += 1;
            let path = relative.join(&item.name);
            if !included(Path::new(""), &path) || (!item.directory && !item.regular) {
                continue;
            }
            if item.directory {
                if depth + 1 >= MAX_DEPTH {
                    truncated = true;
                } else {
                    pending.push((path.clone(), depth + 1));
                }
            }
            let Some(path) = path.to_str() else {
                unreadable += 1;
                continue;
            };
            if filter.is_some_and(|filter| !path.contains(filter)) {
                continue;
            }
            let item = Entry {
                path: path.into(),
                kind: if item.directory { "directory" } else { "file" },
            };
            let size = serde_json::to_vec(&item)?.len() + 1;
            if entries.len() >= MAX_ENTRIES || bytes + size > MAX_BYTES - 1024 {
                truncated = true;
                break 'directories;
            }
            bytes += size;
            entries.push(item);
        }
    }
    let relative = directory;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut listing = Listing {
        directory: if relative.as_os_str().is_empty() {".".into()} else {relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR,"/")},
        filter,
        scope: "Project-relative paths; recursive filename/path substring match, case-sensitive. File contents are not searched. Private settings/state, secrets, build/cache directories and symlinks are excluded.",
        entries,
        truncated: truncated || unreadable > 0,
        unreadable_entries: unreadable,
        limits: Limits { entries: MAX_ENTRIES, visited: MAX_VISITED, bytes: MAX_BYTES, depth: MAX_DEPTH },
    };
    loop {
        let text = serde_json::to_string(&listing)?;
        if text.len() <= MAX_BYTES {
            return Ok(text);
        }
        listing.truncated = true;
        if listing.entries.pop().is_none() {
            bail!("directory metadata exceeds the listing byte budget");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn discovers_paths_and_filters_without_shell_or_contents() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("widgets")).unwrap();
        fs::write(
            root.path().join("widgets/bar.py"),
            "PRIVATE_FILE_BODY_NOT_LISTED",
        )
        .unwrap();
        fs::write(root.path().join("README.md"), "notes").unwrap();
        let text = list(root.path(), root.path(), Some("bar")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["entries"].as_array().unwrap().len(), 1);
        assert_eq!(value["entries"][0]["path"], "widgets/bar.py");
        assert!(!text.contains("PRIVATE_FILE_BODY_NOT_LISTED"));
        assert_eq!(value["truncated"], false);
        assert!(list(root.path(), root.path(), Some("absent"))
            .unwrap()
            .contains("\"entries\":[]"));
    }
    #[cfg(unix)]
    #[test]
    fn excludes_secrets_caches_state_and_symlinks_including_requested_aliases() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("outside.txt"), "secret").unwrap();
        for dir in [".git", "node_modules", ".greenlight", ".stepwright", ".ssh"] {
            fs::create_dir(root.path().join(dir)).unwrap();
            fs::write(root.path().join(dir).join("hidden.txt"), "secret").unwrap();
        }
        for file in [".env", ".env.production", "greenlight.yaml", "key.pem"] {
            fs::write(root.path().join(file), "secret").unwrap();
        }
        symlink(outside.path(), root.path().join("escape")).unwrap();
        symlink("missing", root.path().join("dangling")).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&list(root.path(), root.path(), None).unwrap()).unwrap();
        assert!(value["entries"].as_array().unwrap().is_empty());
        assert!(list(root.path(), outside.path(), None).is_err());
        assert!(list(root.path(), &root.path().join("node_modules"), None).is_err());
    }
    #[test]
    fn bounds_output_and_reports_truncation_and_depth() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..550 {
            fs::write(root.path().join(format!("file-{i:04}.txt")), "").unwrap();
        }
        let text = list(root.path(), root.path(), None).unwrap();
        assert!(text.len() <= MAX_BYTES);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["truncated"], true);
        assert!(value["entries"].as_array().unwrap().len() <= MAX_ENTRIES);
    }
    #[test]
    fn excluded_names_still_count_against_scan_budget_and_deep_scans_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..MAX_VISITED {
            fs::write(root.path().join(format!(".env-{i}")), "").unwrap();
        }
        let value: serde_json::Value =
            serde_json::from_str(&list(root.path(), root.path(), None).unwrap()).unwrap();
        assert!(value["entries"].as_array().unwrap().is_empty());
        assert_eq!(value["truncated"], true);
        let deep = tempfile::tempdir().unwrap();
        let mut directory = deep.path().to_owned();
        for _ in 0..MAX_DEPTH + 1 {
            directory.push("sub");
            fs::create_dir(&directory).unwrap();
        }
        fs::write(directory.join("deep.txt"), "").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&list(deep.path(), deep.path(), None).unwrap()).unwrap();
        assert_eq!(value["truncated"], true);
        assert!(!value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"].as_str().unwrap().ends_with("deep.txt")));
        assert!(list(deep.path(), &directory, None)
            .unwrap()
            .contains("deep.txt"));
    }
}
