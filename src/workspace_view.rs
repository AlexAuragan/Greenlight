//! Read-only projection of authoritative workflow, attempt and ownership records.
use crate::{
    config::{self, Step},
    input_pins::InputManifest,
    session_execution::ExecutionState,
    store::{AttemptInfo, Session, Store},
};
use anyhow::{Context, Result};

type HistoryEntry = (i64, String, String, String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Ready,
    Running,
    Completed,
    Blocked,
    Interrupted,
    Deviated,
    Partial,
}
impl StepState {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Pending => "·",
            Self::Ready => "▷",
            Self::Running => "▶",
            Self::Completed => "✓",
            Self::Blocked => "!",
            Self::Interrupted => "×",
            Self::Deviated => "↻",
            Self::Partial => "?",
        }
    }
}
#[derive(Clone, Debug)]
pub struct StepProgress {
    pub id: String,
    pub state: StepState,
    pub attempts: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryAction {
    Observe,
    Resume,
    RecoverOwner,
    RecoverWrites,
    Review,
    Inspect,
}
impl PrimaryAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Observe => "Observe active run/control; approvals stay with its owner",
            Self::Resume => "i: give instructions and resume · r: restart",
            Self::RecoverOwner => "/recover: recover interrupted owner before resuming",
            Self::RecoverWrites => "CLI recover-writes: reconcile interrupted writes first",
            Self::Review => "2: review artifacts · h: inspect recorded activity",
            Self::Inspect => "e: inspect evidence before deciding how to continue",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Blocker {
    pub event_id: i64,
    pub reason: String,
}
#[derive(Clone, Debug)]
pub struct SessionWorkspace {
    pub goal: String,
    pub steps: Vec<StepProgress>,
    pub attempt: Option<AttemptInfo>,
    pub attempt_number: usize,
    pub next_attempt: bool,
    pub objective: String,
    pub outputs: Option<Vec<String>>,
    pub context_source: String,
    pub inputs: Option<Vec<(String, i64)>>,
    pub approval_events: Vec<i64>,
    pub verification: String,
    pub blocker: Option<Blocker>,
    pub primary: PrimaryAction,
    pub warning: Option<String>,
}

fn observed_state(kind: &str) -> Option<StepState> {
    Some(match kind {
        "step.started" => StepState::Running,
        "step.completed" => StepState::Completed,
        "step.blocked" | "step.error" => StepState::Blocked,
        "step.deviation" => StepState::Deviated,
        _ => return None,
    })
}

impl SessionWorkspace {
    pub fn load(store: &Store, s: &Session, history: &[HistoryEntry]) -> Result<Self> {
        let attempts = store.attempts(s)?;
        let root = store
            .directory
            .parent()
            .and_then(|p| p.parent())
            .context("project root unavailable")?;
        let workflow = config::load_workflow(root, &s.workflow);
        let mut warning = workflow
            .as_ref()
            .err()
            .map(|e| format!("Workflow configuration unavailable: {e:#}"));
        let mut ids = match &workflow {
            Ok(wf) => wf.definition.steps.clone(),
            Err(_) => {
                let mut ids = Vec::new();
                for (_, step, kind, _) in history {
                    if kind == "step.started" && !ids.contains(step) {
                        ids.push(step.clone());
                    }
                }
                if !ids.contains(&s.current_step) {
                    ids.push(s.current_step.clone());
                }
                ids
            }
        };
        // Retain recorded steps even if the workflow file was edited afterward.
        for id in attempts
            .iter()
            .map(|a| &a.step)
            .chain(std::iter::once(&s.current_step))
        {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        let owner = store.execution_state(&s.id)?;
        let writes = store.pending_write_count()?;
        let mut steps = Vec::new();
        for id in ids {
            let records: Vec<_> = attempts.iter().filter(|a| a.step == id).collect();
            let mut state = if let Some(a) = records.last() {
                match a.status.as_str() {
                    "running" => StepState::Running,
                    "completed" => StepState::Completed,
                    "blocked" => StepState::Blocked,
                    "interrupted" => StepState::Interrupted,
                    "deviation" => StepState::Deviated,
                    _ => StepState::Partial,
                }
            } else {
                history
                    .iter()
                    .rev()
                    .find_map(|(_, step, kind, _)| {
                        if *step == id {
                            observed_state(kind)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(StepState::Pending)
            };
            if id == s.current_step {
                state = match (owner, s.status.as_str()) {
                    (ExecutionState::Interrupted, _) => StepState::Interrupted,
                    (_, "blocked") => StepState::Blocked,
                    (_, "ready") => StepState::Ready,
                    (ExecutionState::Live, "running") => StepState::Running,
                    _ => state,
                };
            }
            steps.push(StepProgress {
                id,
                state,
                attempts: records.len(),
            });
        }
        let attempt = attempts
            .iter()
            .rev()
            .find(|a| a.step == s.current_step)
            .cloned();
        let count = attempts.iter().filter(|a| a.step == s.current_step).count();
        let next_attempt = s.status == "ready";
        let definition: Option<Step> = if next_attempt || attempt.is_none() {
            workflow
                .as_ref()
                .ok()
                .and_then(|wf| wf.steps.get(&s.current_step))
                .cloned()
        } else {
            attempt
                .as_ref()
                .and_then(|a| {
                    history
                        .iter()
                        .find(|(id, _, _, _)| *id == a.definition_event)
                })
                .and_then(|(_, _, _, raw)| match serde_yaml::from_str(raw) {
                    Ok(definition) => Some(definition),
                    Err(error) => {
                        warning = Some(format!("Recorded step definition invalid: {error}"));
                        None
                    }
                })
        };
        let context_source = if next_attempt || attempt.is_none() {
            "next-run workflow configuration"
        } else {
            "recorded attempt definition"
        }
        .into();
        let objective = definition
            .as_ref()
            .and_then(|step| {
                step.prompt
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
            })
            .map(|line| line.trim_start_matches('#').trim().to_string())
            .unwrap_or_else(|| "Step objective unavailable; inspect workflow/history".into());
        let outputs = definition
            .as_ref()
            .map(|step| step.outputs.required.clone());
        let manifest = if next_attempt {
            None
        } else {
            history
                .iter()
                .rev()
                .filter(|(_, step, kind, _)| step == &s.current_step && kind == "context.inputs")
                .filter_map(|(_, _, _, raw)| serde_json::from_str::<InputManifest>(raw).ok())
                .find(|manifest| attempt.as_ref().is_some_and(|a| a.id == manifest.attempt))
        };
        let (inputs, approval_events) = manifest
            .map(|m| {
                (
                    Some(
                        m.artifacts
                            .into_iter()
                            .map(|v| (v.name, v.event_id))
                            .collect(),
                    ),
                    m.approval_events,
                )
            })
            .unwrap_or((None, Vec::new()));
        let start = attempt.as_ref().map(|a| a.start_event).unwrap_or(i64::MAX);
        let verification = if next_attempt {
            "not run"
        } else {
            history
                .iter()
                .rev()
                .find_map(|(id, step, kind, _)| {
                    if *id > start && step == &s.current_step {
                        match kind.as_str() {
                            "verification.passed" => Some("passed"),
                            "verification.failed" => Some("failed"),
                            _ => None,
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or("no aggregate verdict recorded")
        }
        .into();
        let restore = history.iter().rev().find(|(_, _, kind, _)| {
            matches!(
                kind.as_str(),
                "workspace.restore-failed" | "workspace.restore-completed"
            )
        });
        let restore_failed =
            restore.is_some_and(|(_, _, kind, _)| kind == "workspace.restore-failed");
        let blocker = restore
            .filter(|(_, _, kind, _)| kind == "workspace.restore-failed")
            .or_else(|| {
                if s.status == "blocked" {
                    history.iter().rev().find(|(_, _, kind, _)| {
                        matches!(
                            kind.as_str(),
                            "step.blocked" | "step.error" | "runner.error" | "runner.recovered"
                        )
                    })
                } else {
                    None
                }
            })
            .map(|(id, _, kind, raw)| Blocker {
                event_id: *id,
                reason: if kind == "workspace.restore-failed" {
                    serde_json::from_str::<serde_json::Value>(raw)
                        .ok()
                        .and_then(|v| v["error"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| raw.clone())
                } else {
                    raw.clone()
                },
            });
        let primary = if owner == ExecutionState::Live {
            PrimaryAction::Observe
        } else if owner == ExecutionState::Interrupted {
            PrimaryAction::RecoverOwner
        } else if writes > 0 {
            PrimaryAction::RecoverWrites
        } else if restore_failed {
            PrimaryAction::Inspect
        } else if s.status == "completed" {
            PrimaryAction::Review
        } else if warning.is_some() {
            PrimaryAction::Inspect
        } else if matches!(s.status.as_str(), "ready" | "blocked") {
            PrimaryAction::Resume
        } else {
            PrimaryAction::Inspect
        };
        Ok(Self {
            goal: s.goal.clone(),
            steps,
            attempt,
            attempt_number: if next_attempt { count + 1 } else { count },
            next_attempt,
            objective,
            outputs,
            context_source,
            inputs,
            approval_events,
            verification,
            blocker,
            primary,
            warning,
        })
    }

    pub fn attempt_label(&self) -> String {
        if self.next_attempt {
            format!("next attempt {}", self.attempt_number)
        } else if let Some(a) = &self.attempt {
            format!(
                "attempt {} · {}",
                self.attempt_number,
                a.id.chars().take(8).collect::<String>()
            )
        } else {
            "attempt ID not recorded (legacy history)".into()
        }
    }
    pub fn stepper(&self, current: &str, width: usize) -> String {
        let labels: Vec<_> = self
            .steps
            .iter()
            .map(|s| format!("{} {}", s.state.marker(), s.id))
            .collect();
        let all = labels.join("  ");
        if ratatui::text::Line::raw(&all).width() <= width {
            return all;
        }
        let current = self.steps.iter().position(|s| s.id == current).unwrap_or(0);
        let mut left = current;
        let mut right = current + 1;
        let render = |left: usize, right: usize| {
            format!(
                "{}{}{}",
                if left > 0 { "… " } else { "" },
                labels[left..right].join("  "),
                if right < labels.len() { " …" } else { "" }
            )
        };
        while left > 0 || right < labels.len() {
            let (l, r) = if left > 0 && (right == labels.len() || current - left <= right - current)
            {
                (left - 1, right)
            } else {
                (left, right + 1)
            };
            if ratatui::text::Line::raw(render(l, r)).width() > width {
                break;
            }
            left = l;
            right = r;
        }
        render(left, right)
    }
    pub fn context(&self) -> String {
        let mut out = format!(
            "Goal: {}\n{}\nContext: {}\nObjective: {}\nRequired outputs: {}\nVerification: {}\n",
            self.goal,
            self.attempt_label(),
            self.context_source,
            self.objective,
            self.outputs
                .as_ref()
                .map(|v| if v.is_empty() {
                    "none declared".into()
                } else {
                    v.join(", ")
                })
                .unwrap_or_else(|| "unavailable".into()),
            self.verification
        );
        if let Some(a) = &self.attempt {
            out.push_str(&format!(
                "Latest attempt ID: {}\nDefinition event: #{}\n",
                a.id, a.definition_event
            ));
        }
        match &self.inputs {
            Some(inputs) => {
                out.push_str("Pinned inputs:\n");
                for (name, id) in inputs {
                    out.push_str(&format!("  {name} — event #{id}\n"));
                }
                if inputs.is_empty() {
                    out.push_str("  none selected\n");
                }
            }
            None => out.push_str("Pinned inputs: not recorded/selected for this attempt\n"),
        }
        if !self.approval_events.is_empty() {
            out.push_str(&format!("Approval events: {:?}\n", self.approval_events));
        }
        if let Some(blocker) = &self.blocker {
            out.push_str(&format!(
                "\nBlocker — evidence #{}:\n{}\n",
                blocker.event_id, blocker.reason
            ));
        }
        if let Some(warning) = &self.warning {
            out.push_str(&format!("\n{warning}\n"));
        }
        out.push_str(&format!("\nNext: {}\n", self.primary.label()));
        out
    }
    pub fn step_summary(&self) -> String {
        self.steps
            .iter()
            .map(|s| {
                format!(
                    "{} {} ({} recorded attempts)",
                    s.state.marker(),
                    s.id,
                    s.attempts
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Store, Session) {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work, review]\n",
        )
        .unwrap();
        for step in ["work", "review"] {
            std::fs::write(
                folder.join(format!("{step}.md")),
                format!("---\nid: {step}\nverification:\n  type: none\n---\nNew configuration objective\n"),
            )
            .unwrap();
        }
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "Fix the project", "work").unwrap();
        (root, store, session)
    }
    fn load(store: &Store, s: &Session) -> SessionWorkspace {
        SessionWorkspace::load(store, s, &store.history(s).unwrap()).unwrap()
    }
    #[test]
    fn recorded_attempt_context_and_verdict_survive_new_config_and_artifacts() {
        let (_root, store, mut s) = fixture();
        store.add_artifact(&s, "work", "input.md", "old").unwrap();
        let old = store.artifacts(&s, "session").unwrap()[0].event_id;
        let first = store
            .begin_attempt(
                &s,
                "work",
                "id: work\nprompt: Saved objective\noutputs:\n  required: [report.md]\n",
                "fixture",
            )
            .unwrap();
        store.event(&s,Some("work"),"context.inputs",&serde_json::json!({"attempt":first,"artifacts":[{"name":"input.md","event_id":old}],"approval_events":[old]}).to_string()).unwrap();
        store
            .event(&s, Some("work"), "verification.failed", "actual failure")
            .unwrap();
        let blocked = store
            .event(&s, Some("work"), "step.blocked", "Needs a fix")
            .unwrap();
        store.add_artifact(&s, "work", "input.md", "new").unwrap();
        store
            .event(&s, Some("work"), "model.response", "verification passed")
            .unwrap();
        s.status = "blocked".into();
        store.save(&s).unwrap();
        let view = load(&store, &s);
        assert_eq!(view.objective, "Saved objective");
        assert_eq!(view.inputs, Some(vec![("input.md".into(), old)]));
        assert_eq!(view.verification, "failed");
        assert_eq!(view.blocker.unwrap().event_id, blocked);
        assert_eq!(view.steps[1].state, StepState::Pending);
        let second = store
            .begin_attempt(&s, "work", "id: work\nprompt: Retry objective\n", "fixture")
            .unwrap();
        let view = load(&store, &s);
        assert_eq!(view.attempt.unwrap().id, second);
        assert_eq!(view.attempt_number, 2);
        assert_eq!(view.verification, "no aggregate verdict recorded");
        assert!(view.inputs.is_none());
    }
    #[test]
    fn live_and_interrupted_ownership_require_distinct_actions() {
        let (_root, store, s) = fixture();
        assert_eq!(load(&store, &s).primary, PrimaryAction::Resume);
        let owner = store.claim_execution(&s.id, None).unwrap();
        let running = store.session(&s.id).unwrap();
        assert_eq!(load(&store, &running).primary, PrimaryAction::Observe);
        drop(owner);
        assert_eq!(load(&store, &running).primary, PrimaryAction::RecoverOwner);
        store.recover_session(&s.id).unwrap();
        assert_eq!(
            load(&store, &store.session(&s.id).unwrap()).primary,
            PrimaryAction::Resume
        );
    }
    #[test]
    fn fork_cutoff_never_inherits_a_later_completion() {
        let (_root, store, mut s) = fixture();
        store
            .begin_attempt(&s, "work", "id: work\nprompt: Original\n", "fixture")
            .unwrap();
        let cutoff = store.active_attempt(&s, "work").unwrap().1;
        store
            .event(&s, Some("work"), "step.completed", "done")
            .unwrap();
        store.fork(&mut s, cutoff, "review").unwrap();
        let view = load(&store, &s);
        assert_eq!(view.steps[0].state, StepState::Partial);
        assert_eq!(view.steps[1].state, StepState::Ready);
    }
    #[test]
    fn legacy_history_and_missing_configuration_are_explicit() {
        let (root, store, mut s) = fixture();
        store
            .event(&s, Some("work"), "step.started", "old run")
            .unwrap();
        s.status = "blocked".into();
        store.save(&s).unwrap();
        std::fs::remove_dir_all(root.path().join("workflows")).unwrap();
        let view = load(&store, &s);
        assert!(view.attempt.is_none());
        assert!(view.attempt_label().contains("legacy"));
        assert!(view.warning.is_some());
        assert_eq!(view.primary, PrimaryAction::Inspect);
        assert_eq!(view.verification, "no aggregate verdict recorded");
    }
    #[test]
    fn unfinished_project_writes_require_reconciliation_even_for_ready_sessions() {
        let (_root, store, s) = fixture();
        store.db.execute("INSERT INTO write_intents(id,session,branch,step,path,before_text,after_text,status) VALUES ('pending',?1,?2,'work','file',NULL,'new','pending')",rusqlite::params![s.id,s.active_branch]).unwrap();
        assert_eq!(load(&store, &s).primary, PrimaryAction::RecoverWrites);
        assert!(store.ensure_no_pending_writes().is_err());
    }
    #[test]
    fn stepper_windows_around_current_step() {
        let (_root, store, s) = fixture();
        let mut view = load(&store, &s);
        view.steps = (0..30)
            .map(|i| StepProgress {
                id: format!("étape{i}"),
                state: StepState::Pending,
                attempts: 0,
            })
            .collect();
        let text = view.stepper("étape17", 78);
        assert!(text.contains("étape17"));
        assert!(text.starts_with('…') && text.ends_with('…'));
        assert!(ratatui::text::Line::raw(text).width() <= 78);
    }
}
