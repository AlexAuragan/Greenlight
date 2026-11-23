//! Optional historical grouping of existing native journal records. No staging or writes.
use crate::{
    changes, resource_limits,
    store::{Session, Store, WorkspaceEdit},
};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Attempt(String),
    Unattributed,
    UserRerun(i64),
    Restore(i64),
    Step,
}
struct Row {
    edit: WorkspaceEdit,
    branch: String,
    step: String,
    source: Source,
}
struct Group {
    branch: String,
    step: String,
    source: Source,
    rows: Vec<usize>,
}
pub struct Review {
    pub session: Session,
    pub cutoff: Option<i64>,
    rows: Vec<Row>,
    latest: BTreeMap<String, i64>,
    attempts: Vec<Group>,
    steps: Vec<Group>,
    limit: usize,
}
impl Store {
    pub fn grouped_edits(&self, selected: &Session) -> Result<Review> {
        let tx = self.db.unchecked_transaction()?;
        let session = self.session(&selected.id)?;
        if session.active_branch != selected.active_branch {
            bail!("Branch changed; refresh before opening grouped review");
        }
        let history = self.history(&session)?;
        let mut edits: BTreeMap<_, _> = self
            .workspace_edits(&session)?
            .into_iter()
            .map(|e| (e.event_id, e))
            .collect();
        let mut query=self.db.prepare("SELECT e.branch, coalesce(e.step,''),
            (SELECT t.id FROM step_attempts t WHERE t.session=e.session AND t.branch=e.branch AND t.step=e.step AND t.start_event<e.id AND (t.end_event IS NULL OR t.end_event>=e.id) ORDER BY t.start_event DESC LIMIT 1)
            FROM events e WHERE e.session=?1 AND e.id=?2")?;
        let mut rows = Vec::new();
        let mut reruns = BTreeMap::new();
        let mut budget = resource_limits::Budget::new(self.limits.materialized_bytes);
        for (event, _, kind, _) in &history {
            let marker = matches!(
                kind.as_str(),
                "tool.rerun"
                    | "tool.rerun-completed"
                    | "runner.started"
                    | "runner.stopped"
                    | "runner.error"
                    | "runner.recovered"
            );
            if !marker && !edits.contains_key(event) {
                continue;
            }
            let (branch, step, attempt): (String, String, Option<String>) = query
                .query_row(params![session.id, event], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
            if marker {
                if kind == "tool.rerun" {
                    reruns.insert(branch, *event);
                } else {
                    reruns.remove(&branch);
                }
                continue;
            }
            let edit = edits.remove(event).context("Journal record disappeared")?;
            let source = if kind == "workspace.restored" {
                Source::Restore(*event)
            } else if let Some(rerun) = reruns.get(&branch) {
                Source::UserRerun(*rerun)
            } else {
                attempt.map(Source::Attempt).unwrap_or(Source::Unattributed)
            };
            budget.charge(
                edit.path.len()
                    + branch.len()
                    + step.len()
                    + 256
                    + edit.before.as_ref().map_or(0, String::len)
                    + edit.after.as_ref().map_or(0, String::len),
            )?;
            rows.push(Row {
                edit,
                branch,
                step,
                source,
            });
        }
        if !edits.is_empty() {
            bail!("Journal records lack visible history; grouped review unavailable");
        }
        let mut attempts: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        let mut steps: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        let mut latest = BTreeMap::new();
        for (index, row) in rows.iter().enumerate() {
            latest.insert(row.edit.path.clone(), row.edit.event_id);
            attempts
                .entry((row.branch.clone(), row.step.clone(), row.source.clone()))
                .or_default()
                .push(index);
            let source = match row.source {
                Source::Attempt(_) | Source::Unattributed => Source::Step,
                _ => row.source.clone(),
            };
            steps
                .entry((row.branch.clone(), row.step.clone(), source))
                .or_default()
                .push(index);
        }
        let collect = |map: BTreeMap<(String, String, Source), Vec<usize>>| {
            let mut groups: Vec<_> = map
                .into_iter()
                .map(|((branch, step, source), rows)| Group {
                    branch,
                    step,
                    source,
                    rows,
                })
                .collect();
            groups.sort_by_key(|g| rows[g.rows[0]].edit.event_id);
            groups
        };
        let review = Review {
            session,
            cutoff: history.last().map(|e| e.0),
            attempts: collect(attempts),
            steps: collect(steps),
            rows,
            latest,
            limit: self.limits.materialized_bytes.min(self.limits.export_bytes),
        };
        drop(query);
        tx.commit()?;
        Ok(review)
    }
}
impl Review {
    pub fn len(&self, by_step: bool) -> usize {
        if by_step {
            self.steps.len()
        } else {
            self.attempts.len()
        }
    }
    pub fn text(&self, by_step: bool, index: usize) -> Result<String> {
        let mut text = resource_limits::Text::new(self.limit);
        text.push_str(&format!("Grouped native edit review · {} · group {} of {}\nSession: {}\nSelected branch: {} · history through {}\nRead-only historical snapshot. Writes are already applied; no staging, permissions, verification or live disk validation. Shell/manual changes are outside this journal.\n\n",if by_step {"by step"} else {"by attempt"},if self.len(by_step)>0{index+1}else{0},self.len(by_step),self.session.id,self.session.active_branch,self.cutoff.map(|e|format!("#{e}")).unwrap_or_else(||"no records".into())));
        let groups = if by_step { &self.steps } else { &self.attempts };
        let Some(group) = groups.get(index) else {
            if groups.is_empty() {
                text.push_str("No native journal writes or restores visible in this snapshot.\n");
                return text
                    .finish()
                    .context("Grouped review exceeds the configured rendering budget");
            }
            bail!("Grouped review selection is unavailable");
        };
        let source = match &group.source {
            Source::Attempt(id) => format!("Recorded attempt {id}"),
            Source::Unattributed => {
                "Unattributed native writes; no durable attempt ID (may include legacy retries)"
                    .into()
            }
            Source::UserRerun(event) => format!("User-requested rerun · recorded marker #{event}"),
            Source::Restore(event) => {
                format!("Explicit workspace restore · event #{event}; not an agent attempt")
            }
            Source::Step => {
                "Recorded step writes across attempts; reruns/restores remain separate".into()
            }
        };
        text.push_str(&format!(
            "Step: {}\nSource branch: {}{}\n{}\n{} journal record(s)\n",
            if group.step.is_empty() {
                "not recorded"
            } else {
                &group.step
            },
            group.branch,
            if group.branch != self.session.active_branch {
                " (inherited)"
            } else {
                ""
            },
            source,
            group.rows.len()
        ));
        if group.source == Source::Step {
            let attempts: std::collections::BTreeSet<&str> = group
                .rows
                .iter()
                .filter_map(|index| {
                    if let Source::Attempt(id) = &self.rows[*index].source {
                        Some(id.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            let unknown = group
                .rows
                .iter()
                .filter(|index| self.rows[**index].source == Source::Unattributed)
                .count();
            text.push_str(&format!(
                "Durable attempt IDs: {}\nUnattributed journal records: {unknown}\n",
                if attempts.is_empty() {
                    "none recorded".into()
                } else {
                    attempts.into_iter().collect::<Vec<_>>().join(", ")
                }
            ));
        }
        let mut paths: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
        for index in &group.rows {
            let row = &self.rows[*index];
            paths.entry(&row.edit.path).or_default().push(row);
        }
        for (path, rows) in paths {
            text.push_str(&format!(
                "\nFile: {}\nJournal events: {}\n",
                path.escape_debug(),
                rows.iter()
                    .map(|r| format!("#{}", r.edit.event_id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            let first = &rows[0].edit;
            let last = &rows[rows.len() - 1].edit;
            if self
                .latest
                .get(path)
                .is_some_and(|event| *event > last.event_id)
            {
                text.push_str("Later journal changes exist; this group is not the latest recorded file state.\n");
            }
            let continuous = rows
                .windows(2)
                .all(|pair| pair[0].edit.after == pair[1].edit.before);
            if continuous {
                text.push_str("Combined net recorded diff for this group:\n");
                append_diff(
                    &mut text,
                    path,
                    first.before.as_deref(),
                    last.after.as_deref(),
                );
            } else {
                text.push_str("History gap or intervening changes outside this group. No combined attribution; each recorded write follows separately.\n");
                for row in rows {
                    text.push_str(&format!("\nExact journal record #{}:\n", row.edit.event_id));
                    append_diff(
                        &mut text,
                        path,
                        row.edit.before.as_deref(),
                        row.edit.after.as_deref(),
                    );
                }
            }
        }
        text.finish().context(
            "Grouped review exceeds the configured rendering budget; no partial diff available",
        )
    }
}
fn append_diff(
    text: &mut resource_limits::Text,
    path: &str,
    before: Option<&str>,
    after: Option<&str>,
) {
    if before == after {
        text.push_str("No net content change within this continuous group.\n");
        return;
    }
    let mut diff = changes::unified(path, before, after.unwrap_or(""));
    if after.is_none() {
        diff = diff.replacen(&format!("+++ b/{path}\n"), "+++ /dev/null\n", 1);
    }
    text.push_str(&diff);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_diffs_are_attempt_specific_or_step_wide_and_snapshot_read_only() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "implement").unwrap();
        let first = store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(
                &s,
                "implement",
                "a.txt",
                Some("original\n"),
                "intermediate\n",
            )
            .unwrap();
        store
            .add_change(&s, "implement", "b.txt", None, "created\n")
            .unwrap();
        store
            .add_change(
                &s,
                "implement",
                "a.txt",
                Some("intermediate\n"),
                "first final\n",
            )
            .unwrap();
        store
            .event(&s, Some("implement"), "step.completed", "")
            .unwrap();
        let second = store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(
                &s,
                "implement",
                "a.txt",
                Some("first final\n"),
                "second final\n",
            )
            .unwrap();
        let before = store.history(&s).unwrap();
        let review = store.grouped_edits(&s).unwrap();
        assert_eq!(review.len(false), 2);
        assert_eq!(review.len(true), 1);
        let text = review.text(false, 0).unwrap();
        assert!(text.contains(&first));
        assert!(!text.contains(&second));
        assert!(text.contains("File: a.txt") && text.contains("File: b.txt"));
        assert!(text.contains("-original\n") && text.contains("+first final\n"));
        assert!(!text.contains("+intermediate\n"));
        assert!(text.contains("Later journal changes exist"));
        let all = review.text(true, 0).unwrap();
        assert!(all.contains("-original\n") && all.contains("+second final\n"));
        assert_eq!(store.history(&s).unwrap(), before);
        assert!(!root.path().join("a.txt").exists());
        store
            .add_change(&s, "implement", "c.txt", None, "later\n")
            .unwrap();
        assert!(!review.text(true, 0).unwrap().contains("File: c.txt"));
        assert!(store
            .grouped_edits(&s)
            .unwrap()
            .text(true, 0)
            .unwrap()
            .contains("File: c.txt"));
    }
    #[test]
    fn step_group_does_not_attribute_intervening_changes_to_its_own_writes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "implement").unwrap();
        store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(&s, "implement", "a.txt", Some("original\n"), "first\n")
            .unwrap();
        store
            .begin_attempt(&s, "other", "definition", "fixture")
            .unwrap();
        store
            .add_change(&s, "other", "a.txt", Some("first\n"), "outside group\n")
            .unwrap();
        store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(&s, "implement", "a.txt", Some("outside group\n"), "last\n")
            .unwrap();
        let review = store.grouped_edits(&s).unwrap();
        let text = review.text(true, 0).unwrap();
        assert!(text.contains("No combined attribution"));
        assert_eq!(text.matches("Exact journal record").count(), 2);
        assert!(text.contains("-outside group\n"));
        assert!(!text.contains("+outside group\n"));
        assert_eq!(review.len(false), 3);
        assert_eq!(review.len(true), 2);
    }
    #[test]
    fn user_reruns_restores_and_unattributed_writes_are_never_claimed_as_attempts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "implement").unwrap();
        store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(&s, "implement", "native.txt", None, "native\n")
            .unwrap();
        store
            .event(&s, Some("implement"), "tool.rerun", "fixture marker")
            .unwrap();
        store
            .add_change(&s, "implement", "rerun.txt", None, "user rerun\n")
            .unwrap();
        store
            .event(&s, Some("implement"), "tool.rerun-completed", "")
            .unwrap();
        store
            .add_restoration(&s, "rerun.txt", Some("user rerun\n"), None)
            .unwrap();
        store
            .event(&s, Some("implement"), "step.completed", "")
            .unwrap();
        store
            .add_change(&s, "implement", "legacy.txt", None, "unattributed\n")
            .unwrap();
        let review = store.grouped_edits(&s).unwrap();
        assert_eq!(review.len(false), 4);
        assert_eq!(review.len(true), 3);
        assert!(review
            .text(false, 1)
            .unwrap()
            .contains("User-requested rerun"));
        let restore = review.text(false, 2).unwrap();
        assert!(restore.contains("Explicit workspace restore"));
        assert!(restore.contains("+++ /dev/null\n"));
        assert!(review
            .text(false, 3)
            .unwrap()
            .contains("no durable attempt ID"));
    }
    #[test]
    fn forks_exclude_future_journal_rows_and_do_not_merge_source_branches() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "implement").unwrap();
        store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(&s, "implement", "a.txt", None, "ancestor\n")
            .unwrap();
        let cutoff = store.history(&s).unwrap().last().unwrap().0;
        store
            .add_change(&s, "implement", "hidden.txt", None, "hidden future\n")
            .unwrap();
        let stale = s.clone();
        store.fork(&mut s, cutoff, "implement").unwrap();
        store
            .begin_attempt(&s, "implement", "definition", "fixture")
            .unwrap();
        store
            .add_change(&s, "implement", "a.txt", Some("ancestor\n"), "child\n")
            .unwrap();
        assert!(store.grouped_edits(&stale).is_err());
        let review = store.grouped_edits(&s).unwrap();
        assert_eq!(review.len(true), 2);
        let parent = review.text(true, 0).unwrap();
        assert!(parent.contains("(inherited)"));
        assert!(!parent.contains("hidden future"));
        assert!(!parent.contains("+child\n"));
        assert!(review.text(true, 1).unwrap().contains("+child\n"));
        let unrelated = store.create("demo", "other", "implement").unwrap();
        assert_eq!(store.grouped_edits(&unrelated).unwrap().len(false), 0);
    }
    #[test]
    fn empty_missing_and_oversized_journal_evidence_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "implement").unwrap();
        assert!(store
            .grouped_edits(&s)
            .unwrap()
            .text(false, 0)
            .unwrap()
            .contains("No native journal"));
        store
            .add_change(&s, "implement", "a.txt", None, "body\n")
            .unwrap();
        store.limits.export_bytes = 10;
        assert!(store.grouped_edits(&s).unwrap().text(false, 0).is_err());
        store.limits.materialized_bytes = 10;
        assert!(store.grouped_edits(&s).is_err());
        store.limits.materialized_bytes = 64 * 1024 * 1024;
        let name: String = store
            .db
            .query_row("SELECT after_blob FROM file_changes LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        std::fs::remove_file(store.directory.join("blobs").join(name)).unwrap();
        assert!(store.grouped_edits(&s).is_err());
    }
}
