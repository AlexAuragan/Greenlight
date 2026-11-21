//! Read-only hash/journal attribution. A matching chain is evidence of agreement,
//! not proof of authorship or recovery of arbitrary workspace contents.
use crate::{
    store::{Session, Store},
    workspace::{self, FileDigest, Manifest},
};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Event = (i64, String, String, String);
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    JournalExplained,
    Unexplained,
    CoverageUnknown,
}
impl Classification {
    fn label(&self) -> &str {
        match self {
            Self::JournalExplained => "Journal explained",
            Self::Unexplained => "Unexplained",
            Self::CoverageUnknown => "Coverage unknown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathAudit {
    pub path: String,
    pub observed_change: String,
    pub classification: Classification,
    pub journal_events: Vec<i64>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Audit {
    pub baseline_event: i64,
    pub observed_event: Option<i64>,
    pub journal_through_event: i64,
    pub step: String,
    pub paths: Vec<PathAudit>,
    pub before_scan: Option<workspace::ScanStats>,
    pub after_scan: Option<workspace::ScanStats>,
    pub manifest_json_bytes: u64,
    pub correlation_us: u64,
    pub warnings: Vec<String>,
}
impl Audit {
    pub fn counts(&self) -> (usize, usize, usize) {
        let count = |c| self.paths.iter().filter(|p| p.classification == c).count();
        (
            count(Classification::JournalExplained),
            count(Classification::Unexplained),
            count(Classification::CoverageUnknown),
        )
    }
    pub fn display(&self) -> String {
        let (explained, unexplained, unknown) = self.counts();
        let mut text=format!("Baseline #{} → {} · step {}\n{explained} journal-explained, {unexplained} unexplained, {unknown} coverage-unknown paths\n",self.baseline_event,self.observed_event.map(|v|format!("recorded #{v}")).unwrap_or_else(||"current read-only scan".into()),self.step);
        for p in &self.paths {
            text.push_str(&format!(
                "  {} · {} · {} · journal {:?}\n    {}\n",
                p.classification.label(),
                p.observed_change.replace('_', " "),
                p.path.escape_debug(),
                p.journal_events,
                p.reason
            ));
        }
        for (label, stats) in [
            ("Before", &self.before_scan),
            ("After/current", &self.after_scan),
        ] {
            match stats {
                Some(s) => text.push_str(&format!(
                    "{label} scan: {} files, {} bytes hashed, {:.3} ms\n",
                    s.files,
                    s.bytes_hashed,
                    s.elapsed_us as f64 / 1000.0
                )),
                None => text.push_str(&format!(
                    "{label} scan metrics unavailable (legacy recording)\n"
                )),
            }
        }
        text.push_str(&format!(
            "Manifest JSON: {} bytes combined; correlation: {:.3} ms\n",
            self.manifest_json_bytes,
            self.correlation_us as f64 / 1000.0
        ));
        for w in &self.warnings {
            text.push_str(&format!("Note: {w}\n"));
        }
        text.push_str("Hash agreement is not authorship proof. No file contents are restored by this report.\n");
        text
    }
}

struct JournalDigest {
    id: i64,
    path: String,
    before: Option<FileDigest>,
    after: Option<FileDigest>,
}
fn digest(bytes: &[u8]) -> FileDigest {
    FileDigest {
        size: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

fn journal(
    store: &Store,
    s: &Session,
    start: i64,
    end: i64,
    history: &[Event],
) -> Result<Vec<JournalDigest>> {
    let records: BTreeMap<_, _> = history.iter().map(|e| (e.0, e)).collect();
    let mut result = Vec::new();
    for (branch, cutoff) in store.lineage(&s.active_branch)? {
        let mut query=store.db.prepare("SELECT event_id,path,before_blob,after_blob,'file.changed' FROM file_changes WHERE session=?1 AND branch=?2 AND event_id>?3 AND event_id<=?4 UNION ALL SELECT event_id,path,before_blob,after_blob,'workspace.restored' FROM restore_changes WHERE session=?1 AND branch=?2 AND event_id>?3 AND event_id<=?4 ORDER BY event_id")?;
        let rows = query.query_map(params![s.id, branch, start, end.min(cutoff)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (id, path, before, after, kind) = row?;
            let record = records
                .get(&id)
                .context("Journal source event is not branch-visible")?;
            if record.2 != kind || record.3 != path {
                bail!("Journal metadata disagrees with event #{id}");
            }
            let blob = |name: Option<String>| -> Result<Option<FileDigest>> {
                name.map(|name| {
                    let p = Path::new(&name);
                    if p.components().count() != 1
                        || !matches!(p.components().next(), Some(std::path::Component::Normal(_)))
                    {
                        bail!("Invalid journal blob name at #{id}");
                    }
                    Ok(digest(
                        &store
                            .directory_fd
                            .open_dir(Path::new("blobs"))?
                            .read(p, store.limits.blob_bytes as u64)
                            .with_context(|| format!("Missing journal evidence at #{id}"))?,
                    ))
                })
                .transpose()
            };
            result.push(JournalDigest {
                id,
                path,
                before: blob(before)?,
                after: blob(after)?,
            });
        }
    }
    result.sort_by_key(|e| e.id);
    if result.windows(2).any(|pair| pair[0].id == pair[1].id) {
        bail!("Duplicate journal evidence in interval");
    }
    Ok(result)
}

pub fn correlate(
    store: &Store,
    s: &Session,
    start: i64,
    end: i64,
    observed: Option<i64>,
    before: &Manifest,
    after: &Manifest,
) -> Result<Audit> {
    let started = std::time::Instant::now();
    let history = store.history(s)?;
    let baseline = history
        .iter()
        .find(|e| e.0 == start)
        .context("Baseline is not branch-visible")?;
    if !matches!(
        baseline.2.as_str(),
        "workspace.manifest.before" | "workspace.manifest.after"
    ) || start > end
    {
        bail!("Invalid workspace audit baseline or interval");
    }
    if !history.iter().any(|e| e.0 == end) {
        bail!("Journal cutoff is not branch-visible");
    }
    let recorded: Manifest = serde_json::from_str(&baseline.3)?;
    if recorded.files != before.files || recorded.skipped != before.skipped {
        bail!("Baseline hashes disagree with recorded manifest");
    }
    let mut warnings = Vec::new();
    let boundary = history.iter().any(|e| {
        e.0 > start && e.0 <= end && matches!(e.2.as_str(), "step.started" | "branch.forked")
    });
    if let Some(id) = observed {
        let record = history
            .iter()
            .find(|e| e.0 == id)
            .context("Observed manifest is not branch-visible")?;
        if id != end
            || start >= id
            || baseline.2 != "workspace.manifest.before"
            || record.2 != "workspace.manifest.after"
            || record.1 != baseline.1
            || boundary
            || history
                .iter()
                .any(|e| e.0 > start && e.0 < end && e.2.starts_with("workspace.manifest."))
        {
            bail!("Workspace manifests do not bound one uninterrupted attempt");
        }
        let recorded: Manifest = serde_json::from_str(&record.3)?;
        if recorded.files != after.files || recorded.skipped != after.skipped {
            bail!("Observed hashes disagree with recorded manifest");
        }
    } else {
        warnings.push("Current scan is not an atomic filesystem snapshot; journal cutoff was captured before scanning. Concurrent writes may remain unexplained.".into());
    }
    if boundary {
        warnings.push(
            "Interval crosses an attempt/fork boundary; attribution is coverage-unknown.".into(),
        );
    }
    if !before.skipped.is_empty() || !after.skipped.is_empty() {
        warnings.push(format!(
            "Skipped paths: {} before, {} after. Excluded paths are outside this audit.",
            before.skipped.len(),
            after.skipped.len()
        ));
    }
    if store.ensure_no_pending_writes().is_err() {
        warnings.push("Unreconciled native writes exist; completed journal evidence may be incomplete. Use recover-writes before further execution.".into());
    }
    let edits = journal(store, s, start, end, &history)?;
    let mut by_path: BTreeMap<&str, Vec<&JournalDigest>> = BTreeMap::new();
    for edit in &edits {
        by_path.entry(&edit.path).or_default().push(edit);
    }
    let known: BTreeSet<_> = edits.iter().map(|e| e.id).collect();
    let mut missing: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for e in history.iter().filter(|e| {
        e.0 > start && e.0 <= end && matches!(e.2.as_str(), "file.changed" | "workspace.restored")
    }) {
        if !known.contains(&e.0) {
            missing.entry(e.3.clone()).or_default().push(e.0);
        }
    }
    let differences: BTreeMap<_, _> = workspace::differences(before, after)
        .into_iter()
        .map(|d| (d.path, d.change))
        .collect();
    let paths: BTreeSet<_> = differences
        .keys()
        .chain(edits.iter().map(|e| &e.path))
        .chain(missing.keys())
        .cloned()
        .collect();
    let mut paths_out = Vec::new();
    for path in paths {
        let entries = by_path.get(path.as_str()).cloned().unwrap_or_default();
        let mut ids: Vec<_> = entries.iter().map(|e| e.id).collect();
        ids.extend(missing.get(&path).into_iter().flatten());
        ids.sort_unstable();
        let (classification, reason) = if boundary
            || !workspace::covered(before, &path)
            || !workspace::covered(after, &path)
        {
            (
                Classification::CoverageUnknown,
                "Path or interval is outside comparable scan coverage.".into(),
            )
        } else if missing.contains_key(&path) {
            (
                Classification::CoverageUnknown,
                "A recorded native edit has no corresponding journal snapshots.".into(),
            )
        } else if entries.is_empty() {
            (
                Classification::Unexplained,
                "No native write/restore journal entry in this interval. Actor is unknown.".into(),
            )
        } else {
            let mut expected = before.files.get(&path).cloned();
            let mut gap = None;
            for e in entries {
                if expected != e.before && gap.is_none() {
                    gap = Some(e.id);
                }
                expected = e.after.clone();
            }
            if let Some(id) = gap {
                (Classification::Unexplained,format!("Journal before-state breaks the hash chain at #{id}; an intervening change is unexplained."))
            } else if expected.as_ref() != after.files.get(&path) {
                (
                    Classification::Unexplained,
                    "Observed final hash differs from the journal's final state.".into(),
                )
            } else {
                (Classification::JournalExplained,"Every journal before/after hash connects the baseline to the observed state; actor identity is not proven.".into())
            }
        };
        paths_out.push(PathAudit {
            path: path.clone(),
            observed_change: differences
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "no_net_change".into()),
            classification,
            journal_events: ids,
            reason,
        });
    }
    Ok(Audit {
        baseline_event: start,
        observed_event: observed,
        journal_through_event: end,
        step: baseline.1.clone(),
        paths: paths_out,
        before_scan: before.scan_stats.clone(),
        after_scan: after.scan_stats.clone(),
        manifest_json_bytes: (serde_json::to_vec(before)?.len() + serde_json::to_vec(after)?.len())
            as u64,
        correlation_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    struct Fixture {
        root: tempfile::TempDir,
        store: Store,
        s: Session,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let s = store.create("demo", "Goal", "work").unwrap();
            Self { root, store, s }
        }
        fn record(&self, phase: &str) -> (i64, Manifest) {
            workspace::record(self.root.path(), &self.store, &self.s, "work", phase).unwrap()
        }
        fn write(&self, path: &str, before: Option<&str>, after: &str) -> i64 {
            fs::write(self.root.path().join(path), after).unwrap();
            self.store
                .add_change(&self.s, "work", path, before, after)
                .unwrap()
        }
        fn audit(&self, before: (i64, Manifest)) -> Audit {
            let after = self.record("after");
            correlate(
                &self.store,
                &self.s,
                before.0,
                after.0,
                Some(after.0),
                &before.1,
                &after.1,
            )
            .unwrap()
        }
    }
    #[test]
    fn continuous_writes_creations_restores_and_external_changes_are_distinguished() {
        let f = Fixture::new();
        for (path, text) in [
            ("tracked.rs", "old"),
            ("external.rs", "old"),
            ("deleted.txt", "delete"),
        ] {
            fs::write(f.root.path().join(path), text).unwrap();
        }
        let before = f.record("before");
        let one = f.write("tracked.rs", Some("old"), "middle");
        let two = f.write("tracked.rs", Some("middle"), "final");
        f.write("empty.txt", None, "");
        fs::remove_file(f.root.path().join("deleted.txt")).unwrap();
        f.store
            .add_restoration(&f.s, "deleted.txt", Some("delete"), None)
            .unwrap();
        fs::write(f.root.path().join("external.rs"), "manual").unwrap();
        fs::write(f.root.path().join("untracked.txt"), "external").unwrap();
        let report = f.audit(before);
        assert_eq!(report.counts(), (3, 2, 0));
        let tracked = report
            .paths
            .iter()
            .find(|p| p.path == "tracked.rs")
            .unwrap();
        assert_eq!(tracked.journal_events, vec![one, two]);
        assert_eq!(tracked.classification, Classification::JournalExplained);
        assert_eq!(
            report
                .paths
                .iter()
                .find(|p| p.path == "empty.txt")
                .unwrap()
                .observed_change,
            "created"
        );
        assert_eq!(
            report
                .paths
                .iter()
                .find(|p| p.path == "deleted.txt")
                .unwrap()
                .observed_change,
            "removed"
        );
        let event_count = f.store.history(&f.s).unwrap().len();
        let status = status(f.root.path(), &f.store, &f.s).unwrap();
        assert!(status.current.paths.is_empty());
        assert_eq!(event_count, f.store.history(&f.s).unwrap().len());
        assert_eq!(
            fs::read_to_string(f.root.path().join("tracked.rs")).unwrap(),
            "final"
        );
        let stats = report.after_scan.unwrap();
        assert_eq!(stats.files, 4);
        assert_eq!(stats.bytes_hashed, 19);
    }
    #[test]
    fn net_zero_edits_do_not_hide_gaps_and_final_hash_must_match() {
        let f = Fixture::new();
        fs::write(f.root.path().join("a"), "A").unwrap();
        let before = f.record("before");
        f.write("a", Some("external"), "A");
        let r = f.audit(before);
        assert_eq!(r.paths[0].observed_change, "no_net_change");
        assert_eq!(r.paths[0].classification, Classification::Unexplained);
        assert!(r.paths[0].reason.contains("breaks"));
        let before = f.record("before");
        f.write("a", Some("A"), "B");
        f.write("a", Some("B"), "A");
        let r = f.audit(before);
        assert_eq!(r.paths[0].classification, Classification::JournalExplained);
        assert_eq!(r.paths[0].observed_change, "no_net_change");
        let before = f.record("before");
        f.write("a", Some("A"), "B");
        fs::write(f.root.path().join("a"), "C").unwrap();
        let r = f.audit(before);
        assert_eq!(r.paths[0].classification, Classification::Unexplained);
        assert!(r.paths[0].reason.contains("final hash"));
    }
    #[test]
    fn coverage_changes_exclusions_and_missing_journal_rows_are_not_explained() {
        let f = Fixture::new();
        fs::write(f.root.path().join("large"), "small").unwrap();
        let before = f.record("before");
        fs::OpenOptions::new()
            .write(true)
            .open(f.root.path().join("large"))
            .unwrap()
            .set_len(21 * 1024 * 1024)
            .unwrap();
        fs::create_dir(f.root.path().join("target")).unwrap();
        f.write("target/cache", None, "cache");
        let missing = f
            .store
            .event(&f.s, Some("work"), "file.changed", "missing.txt")
            .unwrap();
        let r = f.audit(before);
        assert_eq!(r.counts(), (0, 0, 3));
        let path = r.paths.iter().find(|p| p.path == "large").unwrap();
        assert_eq!(path.observed_change, "coverage_changed");
        let path = r.paths.iter().find(|p| p.path == "missing.txt").unwrap();
        assert_eq!(path.journal_events, vec![missing]);
        assert!(path.reason.contains("no corresponding"));
        assert!(!r.warnings.is_empty());
    }
    #[test]
    fn missing_blobs_outside_interval_do_not_poison_current_journal_evidence() {
        let f = Fixture::new();
        let old = f.write("a", None, "old");
        let blob: String = f
            .store
            .db
            .query_row(
                "SELECT after_blob FROM file_changes WHERE event_id=?1",
                [old],
                |r| r.get(0),
            )
            .unwrap();
        fs::remove_file(f.store.directory.join("blobs").join(blob)).unwrap();
        let before = f.record("before");
        f.write("a", Some("old"), "new");
        let r = f.audit(before);
        assert_eq!(r.counts(), (1, 0, 0));
        let before = f.record("before");
        let id = f.write("a", Some("new"), "last");
        let blob: String = f
            .store
            .db
            .query_row(
                "SELECT after_blob FROM file_changes WHERE event_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        fs::remove_file(f.store.directory.join("blobs").join(blob)).unwrap();
        let after = f.record("after");
        assert!(correlate(
            &f.store,
            &f.s,
            before.0,
            after.0,
            Some(after.0),
            &before.1,
            &after.1
        )
        .is_err());
    }
    #[test]
    fn branch_cutoffs_and_retry_boundaries_cannot_supply_unrelated_explanations() {
        let mut f = Fixture::new();
        fs::write(f.root.path().join("a"), "old").unwrap();
        let before = f.record("before");
        let parent = f.write("a", Some("old"), "parent");
        f.record("after");
        f.store.fork(&mut f.s, before.0, "work").unwrap();
        let current = status(f.root.path(), &f.store, &f.s).unwrap();
        assert!(current.latest_step.is_none());
        assert_eq!(current.current.counts(), (0, 0, 1));
        assert!(!current.current.paths[0].journal_events.contains(&parent));
        let after = f.record("after");
        assert!(correlate(
            &f.store,
            &f.s,
            before.0,
            after.0,
            Some(after.0),
            &before.1,
            &after.1
        )
        .is_err());
        let before = f.record("before");
        let child = f.write("a", Some("parent"), "child");
        let r = f.audit(before);
        assert_eq!(r.counts(), (1, 0, 0));
        assert_eq!(r.paths[0].journal_events, vec![child]);
        let before = f.record("before");
        f.store
            .begin_attempt(&f.s, "work", "id: work", "fixture")
            .unwrap();
        let after = f.record("after");
        assert!(correlate(
            &f.store,
            &f.s,
            before.0,
            after.0,
            Some(after.0),
            &before.1,
            &after.1
        )
        .is_err());
        assert!(latest_complete(&f.store, &f.s).is_err());
    }
    #[test]
    fn current_journal_cutoff_is_stable_and_legacy_metrics_remain_unknown() {
        let f = Fixture::new();
        fs::write(f.root.path().join("a"), "old").unwrap();
        let before = f.record("before");
        f.write("a", Some("old"), "new");
        let after = workspace::scan(f.root.path()).unwrap();
        let r = correlate(&f.store, &f.s, before.0, before.0, None, &before.1, &after).unwrap();
        assert_eq!(r.counts(), (0, 1, 0));
        assert!(r.paths[0].journal_events.is_empty());
        let raw = r#"{"files":{},"skipped":[]}"#;
        let legacy: Manifest = serde_json::from_str(raw).unwrap();
        assert!(legacy.scan_stats.is_none());
    }
    #[tokio::test]
    async fn engine_persists_step_correlation_and_metrics_without_extra_model_calls() {
        use crate::{engine::Engine, provider::Script};
        use std::sync::{Arc, Mutex};
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: fixture\npermissions:\n  write: allow\n",
        )
        .unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(
            folder.join("work.md"),
            "---\nid: work\ntools: [write]\nverification:\n  type: none\n---\nWork\n",
        )
        .unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        let script = Arc::new(Mutex::new(Script {
            responses: vec![
                Ok("action: tool\ntool: write\npath: file.rs\ncontent: new source".into()),
                Ok("action: finish".into()),
            ]
            .into(),
            ..Default::default()
        }));
        engine.provider.script = Some(script.clone());
        let s = engine.store.create("demo", "Goal", "work").unwrap();
        engine.run(&s.id, None, None).await.unwrap();
        assert_eq!(engine.store.session(&s.id).unwrap().status, "completed");
        assert_eq!(script.lock().unwrap().requests.len(), 2);
        let history = engine.store.history(&s).unwrap();
        let raw = &history
            .iter()
            .find(|e| e.2 == "workspace.correlation")
            .unwrap()
            .3;
        let audit: Audit = serde_json::from_str(raw).unwrap();
        assert_eq!(audit.counts(), (1, 0, 0));
        assert!(audit.after_scan.is_some());
        assert!(!raw.contains("new source"));
        assert!(
            audit.observed_event.unwrap()
                < history.iter().find(|e| e.2 == "step.completed").unwrap().0
        );
    }
}

pub fn latest_complete(store: &Store, s: &Session) -> Result<Option<Audit>> {
    let history = store.history(s)?;
    let Some(index) = history
        .iter()
        .rposition(|e| e.2 == "workspace.manifest.after")
    else {
        return Ok(None);
    };
    let after = &history[index];
    let before = history[..index]
        .iter()
        .rev()
        .take_while(|e| {
            !matches!(
                e.2.as_str(),
                "step.started" | "branch.forked" | "workspace.manifest.after"
            )
        })
        .find(|e| e.2 == "workspace.manifest.before")
        .context("Completed manifest has no uninterrupted baseline")?;
    correlate(
        store,
        s,
        before.0,
        after.0,
        Some(after.0),
        &serde_json::from_str(&before.3)?,
        &serde_json::from_str(&after.3)?,
    )
    .map(Some)
}

#[derive(Serialize)]
pub struct Status {
    pub latest_step: Option<Audit>,
    pub current: Audit,
}
pub fn status(root: &Path, store: &Store, s: &Session) -> Result<Status> {
    let history = store.history(s)?;
    let end = history.last().context("No session history")?.0;
    let baseline =
        workspace::latest(&history)?.context("No workspace recording; run at least one step")?;
    let current = workspace::scan(root)?;
    Ok(Status {
        latest_step: latest_complete(store, s)?,
        current: correlate(store, s, baseline.0, end, None, &baseline.2, &current)?,
    })
}
