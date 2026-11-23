//! Human artifact revisions and version-specific comments, never workflow approvals.
use crate::store::{Session, Store};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const EDIT_BYTES: usize = 1024 * 1024;
pub const COMMENT_BYTES: usize = 64 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub source: i64,
    pub version: Option<i64>,
    pub author: String,
    pub text: Option<String>,
}
impl Store {
    pub fn edit_artifact(
        &self,
        s: &Session,
        name: &str,
        source: i64,
        latest: i64,
        content: &str,
    ) -> Result<bool> {
        if content.len() > EDIT_BYTES.min(self.limits.blob_bytes) {
            bail!("Artifact edit exceeds the 1 MiB/configured blob limit; no version saved");
        }
        self.with_project_control(s, "artifact-edit", |_| {
            let old = self.artifact_version(s, name, source)?;
            if self.artifact_versions(s, name)?.first().map(|v|v.event_id) != Some(latest) {
                bail!("Latest artifact version changed while editing; reopen and review before saving");
            }
            if old.content == content { return Ok(false); }
            self.publish_artifact(s, &old.step, name, content, Some(&Record {
                name:name.into(), source, version:None, author:"user".into(), text:None,
            }))?;
            Ok(true)
        })
    }
    pub fn comment_artifact(
        &self,
        s: &Session,
        name: &str,
        source: i64,
        text: &str,
    ) -> Result<bool> {
        if text.len() > COMMENT_BYTES {
            bail!("Review comment exceeds 64 KiB; nothing saved");
        }
        if text.trim().is_empty() {
            return Ok(false);
        }
        self.with_project_control(s, "artifact-comment", |_| {
            let old = self.artifact_version(s, name, source)?;
            self.event(
                s,
                Some(&old.step),
                "artifact.comment",
                &serde_json::to_string(&Record {
                    name: name.into(),
                    source,
                    version: None,
                    author: "user".into(),
                    text: Some(text.into()),
                })?,
            )?;
            Ok(true)
        })
    }
    pub fn artifact_comments(&self, s: &Session, name: &str, source: i64) -> Result<String> {
        self.artifact_version(s, name, source)?;
        let mut text = format!("Review comments · {name} · exact version #{source}\nComments are review notes, not approvals or automatic instructions to the agent.\n");
        let mut found = false;
        for (id, _, kind, raw) in self.history(s)? {
            if kind != "artifact.comment" {
                continue;
            }
            let note: Record = serde_json::from_str(&raw)
                .with_context(|| format!("Malformed review comment #{id}"))?;
            if note.name == name && note.source == source {
                found = true;
                text.push_str(&format!(
                    "\nUser comment · event #{id}\n{}\n",
                    note.text.unwrap_or_default()
                ));
            }
        }
        if !found {
            text.push_str("\nNo comments recorded for this version.\n");
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_revision_preserves_original_and_links_parent_without_approving_work() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "plan").unwrap();
        store
            .add_artifact(&s, "plan", "plan.md", "original")
            .unwrap();
        let base = store.artifact_versions(&s, "plan.md").unwrap()[0].event_id;
        assert!(store
            .edit_artifact(&s, "plan.md", base, base, "human revision")
            .unwrap());
        let versions = store.artifact_versions(&s, "plan.md").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].user_source, Some(base));
        assert!(versions[0].attempt.is_none());
        assert!(store
            .artifact_provenance(&s, &versions[0])
            .unwrap()
            .contains(&format!("Edited from exact version #{base}")));
        assert_eq!(
            store.artifact_version(&s, "plan.md", base).unwrap().content,
            "original"
        );
        assert_eq!(
            store.artifacts(&s, "session").unwrap()[0].content,
            "human revision"
        );
        assert_eq!(store.session(&s.id).unwrap().status, s.status);
        assert!(!store
            .history(&s)
            .unwrap()
            .iter()
            .any(|(_, _, kind, _)| kind == "human.approved" || kind.starts_with("verification.")));
        assert!(!store
            .edit_artifact(
                &s,
                "plan.md",
                versions[0].event_id,
                versions[0].event_id,
                "human revision"
            )
            .unwrap());
        assert_eq!(store.artifact_versions(&s, "plan.md").unwrap().len(), 2);
    }
    #[test]
    fn comments_are_exact_version_notes_with_branch_cutoffs_and_no_new_body() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "plan").unwrap();
        store
            .add_artifact(&s, "plan", "plan.md", "original")
            .unwrap();
        let base = store.artifact_versions(&s, "plan.md").unwrap()[0].event_id;
        assert!(!store.comment_artifact(&s, "plan.md", base, " \n").unwrap());
        store
            .comment_artifact(&s, "plan.md", base, "parent-only note")
            .unwrap();
        assert!(store
            .artifact_comments(&s, "plan.md", base)
            .unwrap()
            .contains("parent-only note"));
        let parent = s.clone();
        store.fork(&mut s, base, "plan").unwrap();
        assert!(!store
            .artifact_comments(&s, "plan.md", base)
            .unwrap()
            .contains("parent-only note"));
        store
            .comment_artifact(&s, "plan.md", base, "child-only note")
            .unwrap();
        assert!(!store
            .artifact_comments(&parent, "plan.md", base)
            .unwrap()
            .contains("child-only note"));
        store
            .edit_artifact(&s, "plan.md", base, base, "child revision")
            .unwrap();
        let latest = store.artifact_versions(&s, "plan.md").unwrap()[0].event_id;
        assert!(!store
            .artifact_comments(&s, "plan.md", latest)
            .unwrap()
            .contains("child-only note"));
        assert!(store
            .artifact_comments(&s, "plan.md", base)
            .unwrap()
            .contains("child-only note"));
    }
    #[test]
    fn stale_versions_branches_busy_owners_and_oversized_drafts_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "plan").unwrap();
        store
            .add_artifact(&s, "plan", "plan.md", "original")
            .unwrap();
        let base = store.artifact_versions(&s, "plan.md").unwrap()[0].event_id;
        store
            .add_artifact(&s, "plan", "plan.md", "concurrent revision")
            .unwrap();
        assert!(store
            .edit_artifact(&s, "plan.md", base, base, "stale edit")
            .is_err());
        assert!(store
            .comment_artifact(&s, "plan.md", base, &"x".repeat(COMMENT_BYTES + 1))
            .is_err());
        assert!(store
            .edit_artifact(&s, "plan.md", base, base, &"x".repeat(EDIT_BYTES + 1))
            .is_err());
        let stale = s.clone();
        store.fork(&mut s, base, "plan").unwrap();
        assert!(store
            .edit_artifact(&stale, "plan.md", base, base, "wrong branch")
            .is_err());
        assert!(store
            .comment_artifact(&stale, "plan.md", base, "wrong branch")
            .is_err());
        let other = store.create("demo", "other", "plan").unwrap();
        let owner = store.claim_execution(&other.id, None).unwrap();
        assert!(store
            .edit_artifact(&s, "plan.md", base, base, "busy")
            .is_err());
        assert!(store.comment_artifact(&s, "plan.md", base, "busy").is_err());
        store.finish_execution(&other.id, &owner, None).unwrap();
        drop(owner);
        assert_eq!(store.artifact_versions(&s, "plan.md").unwrap().len(), 1);
        assert!(store
            .edit_artifact(&s, "plan.md", base, base, "fresh")
            .unwrap());
    }
    #[test]
    fn failed_provenance_publication_rolls_back_body_event_and_blob() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "plan").unwrap();
        store
            .add_artifact(&s, "plan", "plan.md", "original")
            .unwrap();
        let base = store.artifact_versions(&s, "plan.md").unwrap()[0].event_id;
        let blobs = std::fs::read_dir(store.directory.join("blobs"))
            .unwrap()
            .count();
        store.db.execute_batch("CREATE TRIGGER reject_review BEFORE INSERT ON events WHEN NEW.kind='artifact.user-edit' BEGIN SELECT RAISE(ABORT, 'fixture publication failure'); END;").unwrap();
        assert!(store
            .edit_artifact(&s, "plan.md", base, base, "not published")
            .is_err());
        assert_eq!(store.artifact_versions(&s, "plan.md").unwrap().len(), 1);
        assert_eq!(
            std::fs::read_dir(store.directory.join("blobs"))
                .unwrap()
                .count(),
            blobs
        );
        assert!(!store
            .history(&s)
            .unwrap()
            .iter()
            .any(|(_, _, kind, _)| kind == "artifact.user-edit"));
    }
}
