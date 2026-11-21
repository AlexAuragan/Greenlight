//! Branch-visible immutable artifact versions. Metadata discovery never opens blobs.
use crate::{
    input_pins::{Approval, InputManifest},
    store::{Artifact, Session, Store},
};
use anyhow::{bail, Context, Result};

#[derive(Clone, Debug)]
pub struct Version {
    pub event_id: i64,
    pub name: String,
    pub step: String,
    pub branch: String,
    pub timestamp: String,
    pub attempt: Option<String>,
    pub definition: Option<i64>,
    blob: String,
}
impl Store {
    pub fn artifact_versions(&self, s: &Session, name: &str) -> Result<Vec<Version>> {
        let mut versions = Vec::new();
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        for (branch, cutoff) in self.lineage(&s.active_branch)? {
            let (count, bytes): (usize, usize) = self.db.query_row("SELECT count(*),coalesce(sum(octet_length(name)+octet_length(step)+octet_length(blob_path)+256),0) FROM artifacts WHERE session=?1 AND branch=?2 AND name=?3 AND event_id<=?4",rusqlite::params![s.id,branch,name,cutoff],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if count > self.limits.history_events.saturating_sub(versions.len()) {
                bail!(
                    "Artifact versions exceed limits.history_events; saved records are unchanged"
                );
            }
            budget.charge(bytes)?;
            let mut query=self.db.prepare("SELECT a.event_id,a.name,a.step,e.timestamp,a.blob_path,
                (SELECT t.id FROM step_attempts t WHERE t.session=a.session AND t.branch=a.branch AND t.step=a.step AND t.start_event<a.event_id AND (t.end_event IS NULL OR t.end_event>=a.event_id) ORDER BY t.start_event DESC LIMIT 1),
                (SELECT t.definition_event FROM step_attempts t WHERE t.session=a.session AND t.branch=a.branch AND t.step=a.step AND t.start_event<a.event_id AND (t.end_event IS NULL OR t.end_event>=a.event_id) ORDER BY t.start_event DESC LIMIT 1)
                FROM artifacts a JOIN events e ON e.id=a.event_id WHERE a.session=?1 AND a.branch=?2 AND a.name=?3 AND a.event_id<=?4 ORDER BY a.event_id")?;
            let rows = query.query_map(rusqlite::params![s.id, branch, name, cutoff], |r| {
                Ok(Version {
                    event_id: r.get(0)?,
                    name: r.get(1)?,
                    step: r.get(2)?,
                    timestamp: r.get(3)?,
                    blob: r.get(4)?,
                    attempt: r.get(5)?,
                    definition: r.get(6)?,
                    branch: branch.clone(),
                })
            })?;
            versions.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        versions.sort_by_key(|v| std::cmp::Reverse(v.event_id));
        Ok(versions)
    }
    pub fn artifact_version(&self, s: &Session, name: &str, event: i64) -> Result<Artifact> {
        let version = self
            .artifact_versions(s, name)?
            .into_iter()
            .find(|v| v.event_id == event)
            .context("artifact version is not visible on this session's active branch")?;
        let content = self.read_blob(&version.blob).with_context(|| {
            format!(
                "Artifact {} event #{} body unavailable",
                version.name, event
            )
        })?;
        Ok(Artifact {
            name: version.name,
            step: version.step,
            event_id: event,
            content,
        })
    }
    pub fn artifact_provenance(&self, s: &Session, v: &Version) -> Result<String> {
        // Revalidate visibility rather than accepting a stale version from another branch.
        if !self
            .artifact_versions(s, &v.name)?
            .iter()
            .any(|item| item.event_id == v.event_id)
        {
            bail!("artifact version is no longer visible on this branch")
        }
        let mut text=format!("Artifact: {}\nCreation event: #{}\nCreated: {} UTC\nProducing step: {}\nSource branch: {}{}\nProducer attempt: {}\nDefinition event: {}\n",v.name,v.event_id,v.timestamp,v.step,v.branch,if v.branch!=s.active_branch{" (inherited)"}else{""},v.attempt.as_deref().unwrap_or("not recorded / outside an attempt"),v.definition.map(|id|format!("#{id}")).unwrap_or_else(||"not recorded".into()));
        let mut consumers = Vec::new();
        let mut approvals = Vec::new();
        let mut warnings = Vec::new();
        for (id, step, kind, raw) in self.history(s)? {
            match kind.as_str() {
                "context.inputs" => match serde_json::from_str::<InputManifest>(&raw) {
                    Ok(m)
                        if m.artifacts
                            .iter()
                            .any(|a| a.name == v.name && a.event_id == v.event_id) =>
                    {
                        consumers.push(format!("event #{id} · step {step} · attempt {}", m.attempt))
                    }
                    Ok(_) => {}
                    Err(_) => warnings.push(format!(
                        "Malformed input manifest #{id}; references unavailable"
                    )),
                },
                "human.approved" if !raw.is_empty() => match serde_json::from_str::<Approval>(&raw)
                {
                    Ok(a)
                        if a.inputs
                            .artifacts
                            .iter()
                            .any(|item| item.name == v.name && item.event_id == v.event_id) =>
                    {
                        approvals.push(format!(
                            "event #{id} · step {step} · input manifest #{}",
                            a.input_manifest
                        ))
                    }
                    Ok(_) => {}
                    Err(_) => warnings.push(format!(
                        "Malformed human approval #{id}; references unavailable"
                    )),
                },
                _ => {}
            }
        }
        text.push_str(&format!(
            "\nRecorded consumers:\n{}\n\nHuman approvals of this exact version:\n{}\n",
            if consumers.is_empty() {
                "none recorded".into()
            } else {
                consumers.join("\n")
            },
            if approvals.is_empty() {
                "none recorded (legacy approvals without version IDs cannot be attributed)".into()
            } else {
                approvals.join("\n")
            }
        ));
        if !warnings.is_empty() {
            text.push_str(&format!("\n{}\n", warnings.join("\n")));
        }
        Ok(text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Content,
    Provenance,
    Diff,
}
pub struct Browser {
    pub session: String,
    pub branch: String,
    pub name: String,
    pub versions: Vec<Version>,
    pub index: usize,
    pub reading: bool,
    pub mode: Mode,
    pub text: String,
    pub error: Option<String>,
}
impl Browser {
    pub fn open(store: &Store, s: &Session, name: &str) -> Result<Self> {
        let versions = store.artifact_versions(s, name)?;
        if versions.is_empty() {
            bail!("No visible versions for {name}")
        }
        let mut browser = Self {
            session: s.id.clone(),
            branch: s.active_branch.clone(),
            name: name.into(),
            versions,
            index: 0,
            reading: false,
            mode: Mode::Content,
            text: String::new(),
            error: None,
        };
        browser.load(store, s);
        Ok(browser)
    }
    pub fn load(&mut self, store: &Store, s: &Session) {
        let result = (|| -> Result<String> {
            let current = self
                .versions
                .get(self.index)
                .context("No version selected")?;
            match self.mode {
                Mode::Provenance => store.artifact_provenance(s, current),
                Mode::Content => Ok(store
                    .artifact_version(s, &self.name, current.event_id)?
                    .content),
                Mode::Diff => {
                    let previous = self
                        .versions
                        .get(self.index + 1)
                        .context("First visible version: no predecessor to compare")?;
                    let old = store.artifact_version(s, &self.name, previous.event_id)?;
                    let new = store.artifact_version(s, &self.name, current.event_id)?;
                    if old.content == new.content {
                        return Ok(format!(
                            "Events #{} → #{}: identical content",
                            previous.event_id, current.event_id
                        ));
                    }
                    // Avoid expensive diffs of arbitrarily large stored artifacts. Never diff clipped text.
                    if old.content.len() > 256 * 1024 || new.content.len() > 256 * 1024 {
                        bail!("Diff exceeds the 256 KiB per-version limit; export the original versions for an external comparison")
                    }
                    Ok(format!(
                        "Events #{} → #{}\n\n{}",
                        previous.event_id,
                        current.event_id,
                        crate::changes::unified(&self.name, Some(&old.content), &new.content)
                    ))
                }
            }
        })();
        match result {
            Ok(text) => {
                self.text = text;
                self.error = None;
            }
            Err(error) => {
                self.text.clear();
                self.error = Some(format!("{error:#}"));
            }
        }
    }
    pub fn refresh(&mut self, store: &Store, s: &Session) -> Result<()> {
        let selected = self.versions.get(self.index).map(|v| v.event_id);
        self.versions = store.artifact_versions(s, &self.name)?;
        self.index = selected
            .and_then(|id| self.versions.iter().position(|v| v.event_id == id))
            .context("Selected artifact version is no longer visible")?;
        // Content and diffs are immutable. Only provenance can gain new references.
        if self.mode == Mode::Provenance {
            self.load(store, s);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fork_cutoffs_exclude_future_and_unrelated_versions() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "work").unwrap();
        store
            .add_artifact(&s, "work", "report.md", "ancestor")
            .unwrap();
        let ancestor = store.artifacts(&s, "session").unwrap()[0].event_id;
        store
            .add_artifact(&s, "work", "report.md", "future")
            .unwrap();
        let hidden = store.artifacts(&s, "session").unwrap()[0].event_id;
        store.fork(&mut s, ancestor, "work").unwrap();
        store
            .add_artifact(&s, "work", "report.md", "child")
            .unwrap();
        let versions = store.artifact_versions(&s, "report.md").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[1].event_id, ancestor);
        assert!(store.artifact_version(&s, "report.md", hidden).is_err());
        let other = store.create("demo", "other", "work").unwrap();
        assert!(store
            .artifact_version(&other, "report.md", ancestor)
            .is_err());
        let provenance = store.artifact_provenance(&s, &versions[1]).unwrap();
        assert!(provenance.contains("(inherited)"));
        let mut browser = Browser::open(&store, &s, "report.md").unwrap();
        browser.mode = Mode::Diff;
        browser.load(&store, &s);
        assert!(browser.text.contains("-ancestor"));
        assert!(browser.text.contains("+child"));
        assert!(!browser.text.contains("future"));
    }
    #[test]
    fn exact_version_provenance_does_not_transfer_approvals_to_new_content() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let producer = store
            .begin_attempt(&s, "work", "id: work", "fixture")
            .unwrap();
        store
            .add_artifact(&s, "work", "report.md", "first")
            .unwrap();
        let old = store.artifacts(&s, "session").unwrap();
        store
            .event(&s, Some("work"), "step.completed", "done")
            .unwrap();
        store
            .begin_attempt(&s, "review", "id: review", "fixture")
            .unwrap();
        let manifest = store.record_input_manifest(&s, "review", &old).unwrap();
        store
            .approve_inputs(&s, "review", manifest, &old, || Ok(old.clone()))
            .unwrap();
        store
            .add_artifact(
                &s,
                "work",
                "report.md",
                "replacement outside producer attempt",
            )
            .unwrap();
        let versions = store.artifact_versions(&s, "report.md").unwrap();
        assert_eq!(versions[1].attempt.as_deref(), Some(producer.as_str()));
        assert!(versions[0].attempt.is_none());
        let old_text = store.artifact_provenance(&s, &versions[1]).unwrap();
        assert!(old_text.contains(&format!("event #{manifest} · step review")));
        assert!(old_text.contains("input manifest #"));
        let new_text = store.artifact_provenance(&s, &versions[0]).unwrap();
        assert!(new_text.contains("none recorded (legacy"));
        assert!(!new_text.contains(&format!("event #{manifest} · step review")));
    }
    #[test]
    fn missing_old_blobs_leave_latest_content_and_metadata_available() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        store.add_artifact(&s, "work", "report.md", "old").unwrap();
        let old = store.artifact_versions(&s, "report.md").unwrap()[0].clone();
        store.add_artifact(&s, "work", "report.md", "new").unwrap();
        std::fs::remove_file(store.directory.join("blobs").join(&old.blob)).unwrap();
        assert_eq!(store.artifacts(&s, "session").unwrap()[0].content, "new");
        assert_eq!(store.artifact_versions(&s, "report.md").unwrap().len(), 2);
        assert!(store
            .artifact_version(&s, "report.md", old.event_id)
            .is_err());
        assert!(store.artifact_provenance(&s, &old).is_ok());
        let mut browser = Browser::open(&store, &s, "report.md").unwrap();
        browser.mode = Mode::Diff;
        browser.load(&store, &s);
        assert!(browser.error.as_ref().unwrap().contains("body unavailable"));
        browser.index = 1;
        browser.mode = Mode::Provenance;
        browser.load(&store, &s);
        assert!(browser.error.is_none());
        assert!(browser.text.contains("Creation event:"));
    }
    #[test]
    fn selected_version_stays_pinned_when_new_versions_arrive() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        store.add_artifact(&s, "work", "report.md", "one").unwrap();
        let mut browser = Browser::open(&store, &s, "report.md").unwrap();
        let selected = browser.versions[0].event_id;
        store.add_artifact(&s, "work", "report.md", "two").unwrap();
        browser.refresh(&store, &s).unwrap();
        assert_eq!(browser.versions[browser.index].event_id, selected);
        assert_eq!(browser.text, "one");
        browser.mode = Mode::Diff;
        browser.load(&store, &s);
        assert!(browser.error.as_ref().unwrap().contains("no predecessor"));
        browser.index = 0;
        browser.load(&store, &s);
        assert!(browser.text.contains("-one") && browser.text.contains("+two"));
    }
    #[test]
    fn malformed_references_are_reported_and_large_diffs_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        store
            .add_artifact(&s, "work", "report.md", "small")
            .unwrap();
        store
            .add_artifact(&s, "work", "report.md", &"L".repeat(256 * 1024 + 1))
            .unwrap();
        store
            .event(&s, Some("work"), "context.inputs", "malformed")
            .unwrap();
        let mut browser = Browser::open(&store, &s, "report.md").unwrap();
        browser.mode = Mode::Provenance;
        browser.load(&store, &s);
        assert!(browser.text.contains("Malformed input manifest"));
        browser.mode = Mode::Diff;
        browser.load(&store, &s);
        assert!(browser.error.as_ref().unwrap().contains("256 KiB"));
        assert!(browser.text.is_empty());
    }
}

#[cfg(test)]
mod diff_tests {
    use super::*;
    #[test]
    fn identical_versions_and_eof_only_changes_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        store.add_artifact(&s, "work", "report.md", "line").unwrap();
        store
            .add_artifact(&s, "work", "report.md", "line\n")
            .unwrap();
        let mut browser = Browser::open(&store, &s, "report.md").unwrap();
        browser.mode = Mode::Diff;
        browser.load(&store, &s);
        assert!(browser.text.contains("No newline at end of file"));
        store
            .add_artifact(&s, "work", "report.md", "line\n")
            .unwrap();
        let mut browser = Browser::open(&store, &s, "report.md").unwrap();
        browser.mode = Mode::Diff;
        browser.load(&store, &s);
        assert!(browser.text.contains("identical content"));
        assert!(browser.error.is_none());
    }
}
