//! Optional post-cleanup metadata scan. No contents, symlink traversal, quotas,
//! source edits or guesses about bytes written / total device occupancy.
use crate::anchored_fs::Dir;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Footprint {
    pub entries: usize,
    pub unique_regular_files: usize,
    pub logical_regular_file_bytes: u64,
    pub allocated_regular_file_bytes: u64,
    pub skipped_symlinks: usize,
    pub skipped_special_files: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub inspection_ms: f64,
    pub footprint: Option<Footprint>,
    pub unavailable_reason: Option<String>,
}
pub fn observe(cache: &Dir) -> Observation {
    let started = Instant::now();
    let result = scan(cache, 20_000, Duration::from_secs(2));
    let (footprint, unavailable_reason) = match result {
        Ok(footprint) => (Some(footprint), None),
        Err(error) => (
            None,
            Some(format!(
                "Private-cache metadata inspection incomplete; no total claimed: {error:#}"
            )),
        ),
    };
    Observation {
        inspection_ms: started.elapsed().as_secs_f64() * 1000.0,
        footprint,
        unavailable_reason,
    }
}
fn scan(cache: &Dir, max_entries: usize, budget: Duration) -> Result<Footprint> {
    let started = Instant::now();
    let mut pending = vec![(PathBuf::new(), 0)];
    let mut seen = BTreeSet::new();
    let mut queued_path_bytes = 0usize;
    let mut footprint = Footprint {
        entries: 0,
        unique_regular_files: 0,
        logical_regular_file_bytes: 0,
        allocated_regular_file_bytes: 0,
        skipped_symlinks: 0,
        skipped_special_files: 0,
    };
    while let Some((relative, depth)) = pending.pop() {
        queued_path_bytes -= relative.as_os_str().len();
        if depth > 64 || started.elapsed() >= budget {
            bail!("Cache inspection depth/time budget reached");
        }
        let directory = cache.open_dir(&relative)?;
        for entry in directory.entries()? {
            if footprint.entries >= max_entries || started.elapsed() >= budget {
                bail!("Cache inspection entry/time budget reached");
            }
            let entry = entry?;
            footprint.entries += 1;
            let metadata = directory.metadata(Path::new(&entry.name))?;
            if metadata.is_dir() {
                let next = relative.join(entry.name);
                queued_path_bytes = queued_path_bytes
                    .checked_add(next.as_os_str().len())
                    .context("Cache path-size counter overflow")?;
                if queued_path_bytes > 8 * 1024 * 1024 {
                    bail!("Cache inspection queued-path memory budget reached");
                }
                pending.push((next, depth + 1));
            } else if metadata.is_symlink() {
                footprint.skipped_symlinks += 1;
            } else if metadata.is_file() {
                if seen.insert((metadata.dev(), metadata.ino())) {
                    footprint.unique_regular_files += 1;
                    footprint.logical_regular_file_bytes = footprint
                        .logical_regular_file_bytes
                        .checked_add(metadata.len())
                        .context("Cache logical-size counter overflow")?;
                    let allocated = metadata
                        .blocks()
                        .checked_mul(512)
                        .context("Cache allocation counter overflow")?;
                    footprint.allocated_regular_file_bytes = footprint
                        .allocated_regular_file_bytes
                        .checked_add(allocated)
                        .context("Cache allocation total overflow")?;
                }
            } else {
                footprint.skipped_special_files += 1;
            }
        }
    }
    if started.elapsed() >= budget {
        bail!("Cache inspection time budget reached");
    }
    Ok(footprint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::{fs::symlink, net::UnixListener},
    };
    #[test]
    fn sparse_files_hardlinks_links_and_sockets_are_not_conflated() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "not inspected").unwrap();
        fs::write(root.path().join("data"), b"data").unwrap();
        fs::hard_link(root.path().join("data"), root.path().join("alias")).unwrap();
        let sparse = fs::File::create(root.path().join("sparse")).unwrap();
        sparse.set_len(1024 * 1024).unwrap();
        symlink(outside.path(), root.path().join("outside")).unwrap();
        let _socket = UnixListener::bind(root.path().join("socket")).unwrap();
        let expected_allocated = (sparse.metadata().unwrap().blocks()
            + fs::metadata(root.path().join("data")).unwrap().blocks())
            * 512;
        let observed = observe(&Dir::open(root.path()).unwrap());
        let footprint = observed.footprint.unwrap();
        assert_eq!(footprint.entries, 5);
        assert_eq!(footprint.unique_regular_files, 2);
        assert_eq!(footprint.logical_regular_file_bytes, 1024 * 1024 + 4);
        assert_eq!(footprint.allocated_regular_file_bytes, expected_allocated);
        assert_eq!(footprint.skipped_symlinks, 1);
        assert_eq!(footprint.skipped_special_files, 1);
        assert!(observed.unavailable_reason.is_none());
    }
    #[test]
    fn empty_is_measured_zero_but_budget_failure_has_no_total() {
        let root = tempfile::tempdir().unwrap();
        let anchored = Dir::open(root.path()).unwrap();
        assert_eq!(
            observe(&anchored)
                .footprint
                .unwrap()
                .logical_regular_file_bytes,
            0
        );
        fs::write(root.path().join("one"), "one").unwrap();
        fs::write(root.path().join("two"), "two").unwrap();
        assert!(scan(&anchored, 1, Duration::from_secs(2)).is_err());
        assert!(scan(&anchored, 20_000, Duration::ZERO).is_err());
    }
    #[test]
    fn excessive_depth_returns_unavailable_instead_of_partial_totals() {
        let root = tempfile::tempdir().unwrap();
        let mut path = root.path().to_path_buf();
        for _ in 0..65 {
            path.push("nested");
            fs::create_dir(&path).unwrap();
        }
        let observation = observe(&Dir::open(root.path()).unwrap());
        assert!(observation.footprint.is_none());
        assert!(observation.unavailable_reason.unwrap().contains("budget"));
    }

    #[test]
    fn pinned_cache_does_not_follow_replaced_root() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("cache");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("data"), "data").unwrap();
        let anchored = Dir::open(&root).unwrap();
        fs::rename(&root, parent.path().join("old")).unwrap();
        symlink("/etc", &root).unwrap();
        assert_eq!(
            observe(&anchored)
                .footprint
                .unwrap()
                .logical_regular_file_bytes,
            4
        );
    }
}
