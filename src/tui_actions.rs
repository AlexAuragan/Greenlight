//! Typed TUI actions use the same Engine and guarded restore/fork operations
//! as the CLI. TUI approval prompts are transported to the foreground thread.
use crate::{
    config::{self, LoadedWorkflow},
    engine::Engine,
    store::{Session, Store},
    tools::ApprovalPrompt,
};
use anyhow::{bail, Result};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

#[derive(Debug)]
pub enum RunnerError {
    Project(crate::project_execution::ProjectConflict),
    Other(String),
}
impl From<anyhow::Error> for RunnerError {
    fn from(error: anyhow::Error) -> Self {
        match error.downcast::<crate::project_execution::ProjectConflict>() {
            Ok(conflict) => Self::Project(conflict),
            Err(error) => Self::Other(format!("{error:#}")),
        }
    }
}
impl From<String> for RunnerError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}
impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Project(conflict) => std::fmt::Display::fmt(conflict, f),
            Self::Other(message) => f.write_str(message),
        }
    }
}
pub struct Runner {
    pub tool_rerun: bool,
    pub live: Option<crate::provider::stream::Preview>,
    pub cancel_retry: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub session_id: Option<String>,
    pub prompts: Receiver<ApprovalPrompt>,
    pub finished: Receiver<Result<(), RunnerError>>,
    pub answers: Sender<String>,
}

pub fn resume(root: PathBuf, session_id: String, instruction: String) -> Runner {
    start(root, session_id, Work::Resume(instruction))
}

pub fn rerun(root: PathBuf, session_id: String, branch: String, request: i64) -> Runner {
    start(root, session_id, Work::Rerun(branch, request))
}

pub fn resume_saved(root: PathBuf, session_id: String) -> Runner {
    start(root, session_id, Work::Saved)
}

pub fn resume_route(root: PathBuf, session_id: String, message: String) -> Runner {
    start(root, session_id, Work::Route(message))
}

enum Work {
    Route(String),
    Resume(String),
    Rerun(String, i64),
    Saved,
}
fn start(root: PathBuf, session_id: String, work: Work) -> Runner {
    let (prompt_tx, prompts) = mpsc::channel();
    let (answers, answer_rx) = mpsc::channel();
    let (finished_tx, finished) = mpsc::channel();
    let owner_session = session_id.clone();
    let is_rerun = matches!(&work, Work::Rerun(..));
    let cancel_retry = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = cancel_retry.clone();

    let live = crate::provider::stream::Preview::default();
    let worker_live = live.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut engine = Engine::open(&root)?;
            engine.interactive_provider_retry = true;
            engine.tools.stop = worker_cancel.clone();
            engine.provider.retry_cancel = worker_cancel;
            worker_live.lock().unwrap().connection = Some(engine.provider.connection_info());
            engine.provider.live = Some(worker_live);
            engine.tools.use_tui(prompt_tx, answer_rx);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            match work {
                Work::Rerun(branch, request) => {
                    runtime.block_on(engine.rerun_tool(&session_id, &branch, request))
                }
                Work::Resume(instruction) => {
                    runtime.block_on(engine.run(&session_id, Some(&instruction), None))
                }
                Work::Saved => runtime.block_on(engine.resume_saved(&session_id)),
                Work::Route(message) => {
                    runtime.block_on(engine.run_manager_route(&session_id, &message))
                }
            }
        })();
        let _ = finished_tx.send(result.map_err(RunnerError::from));
    });

    Runner {
        tool_rerun: is_rerun,
        live: if is_rerun { None } else { Some(live) },
        prompts,
        finished,
        answers,
        session_id: Some(owner_session),
        cancel_retry: Some(cancel_retry),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestartCandidate {
    pub step: String,
    /// Restore to this visible event, immediately before the selected execution.
    pub prior_event: i64,
}

pub fn restart_candidates(
    store: &Store,
    session: &Session,
    workflow: &LoadedWorkflow,
) -> Result<Vec<RestartCandidate>> {
    let history = store.history(session)?;
    let mut candidates = Vec::new();
    for step in &workflow.definition.steps {
        let Some(start) = history
            .iter()
            .position(|(_, event_step, kind, _)| event_step == step && kind == "step.started")
        else {
            continue;
        };
        // session.started is the earliest safe checkpoint for the first step.
        let prior_event = if start == 0 {
            history[0].0
        } else {
            history[start - 1].0
        };
        candidates.push(RestartCandidate {
            step: step.clone(),
            prior_event,
        });
    }
    Ok(candidates)
}

/// Every visible attempt, including retries, in execution order. Manager-directed
/// restarts retain their canonical first-visit policy in restart_candidates.
pub fn historical_restart_candidates(
    store: &Store,
    session: &Session,
    workflow: &LoadedWorkflow,
) -> Result<Vec<RestartCandidate>> {
    let history = store.history(session)?;
    Ok(history
        .iter()
        .enumerate()
        .filter(|(_, e)| e.2 == "step.started" && workflow.definition.steps.contains(&e.1))
        .map(|(index, e)| RestartCandidate {
            step: e.1.clone(),
            prior_event: history[index.saturating_sub(1)].0,
        })
        .collect())
}
pub fn restart_label(choices: &[RestartCandidate], index: usize) -> String {
    let choice = &choices[index];
    let ordinal = choices[..=index]
        .iter()
        .filter(|c| c.step == choice.step)
        .count();
    format!(
        "{} · run {} · checkpoint #{}",
        choice.step.escape_debug(),
        ordinal,
        choice.prior_event
    )
}
/// Compare captured hashes only: missing scans or changed coverage are not equality.
pub fn compare_branches(
    store: &Store,
    session: &Session,
    left: &str,
    right: &str,
) -> Result<String> {
    let branches = store.branches(&session.id)?;
    if ![left, right]
        .iter()
        .all(|id| branches.iter().any(|b| b.id == *id))
    {
        bail!("Both branches must belong to the selected session");
    }
    let mut a = session.clone();
    a.active_branch = left.into();
    let mut b = session.clone();
    b.active_branch = right.into();
    let ah = store.history(&a)?;
    let bh = store.history(&b)?;
    let mut report=format!("Recorded branch comparison\nLeft: {left} · through event #{}\nRight: {right} · through event #{}\nHashes describe captured files, not current disk contents or restorable bodies.\n",ah.last().map(|e|e.0).unwrap_or(0),bh.last().map(|e|e.0).unwrap_or(0));
    for (label, id) in [("Left", left), ("Right", right)] {
        let branch = branches.iter().find(|b| b.id == id).unwrap();
        report.push_str(&format!(
            "{label} parent: {} · fork checkpoint: {:?}\n",
            branch.parent.as_deref().unwrap_or("root"),
            branch.fork_event
        ));
    }
    match (crate::workspace::latest(&ah)?,crate::workspace::latest(&bh)?) {
        (Some((aid,_,a)),Some((bid,_,b)))=> {
            report.push_str(&format!("Workspace scans: #{aid} → #{bid}\n"));
            let differences=crate::workspace::differences(&a,&b);
            if differences.is_empty() {report.push_str("No hash differences within captured coverage.\n");}
            for change in differences {report.push_str(&format!("{} · {}\n",change.change,change.path.escape_debug()));}
            report.push_str(&format!("Omitted paths: left {} · right {}. Exclusions are outside this comparison.\n",a.skipped.len(),b.skipped.len()));
        }
        _=>report.push_str("Comparison unavailable: at least one branch has no visible workspace scan. This does not establish equality.\n"),
    }
    crate::resource_limits::Budget::new(store.limits.materialized_bytes).charge(report.len())?;
    Ok(report)
}

pub fn candidates_for_session(
    root: &std::path::Path,
    store: &Store,
    session: &Session,
) -> Result<Vec<RestartCandidate>> {
    if session.status == "running" {
        bail!("cannot rewind a running session");
    }
    let workflow = config::load_workflow(root, &session.workflow)?;
    historical_restart_candidates(store, session, &workflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn restart_uses_event_before_first_step_run() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("example", "goal", "plan").unwrap();
        let init = store.history(&session).unwrap()[0].0;
        store
            .event(&session, Some("plan"), "step.started", "")
            .unwrap();
        let plan_done = store
            .event(&session, Some("plan"), "step.completed", "")
            .unwrap();
        store
            .event(&session, Some("implement"), "step.started", "")
            .unwrap();
        store
            .event(&session, Some("implement"), "step.started", "retry")
            .unwrap();
        let workflow = LoadedWorkflow {
            definition: crate::config::Workflow {
                id: "example".into(),
                title: "Example".into(),
                start: "plan".into(),
                steps: vec!["plan".into(), "implement".into()],
                defaults: crate::config::StepDefaults::default(),
            },
            steps: std::collections::HashMap::new(),
            folder: root.path().to_path_buf(),
        };
        let historical = historical_restart_candidates(&store, &session, &workflow).unwrap();
        assert_eq!(historical.len(), 3);
        assert_eq!(historical[0].prior_event, init);
        assert_eq!(historical[1].prior_event, plan_done);
        assert!(historical[2].prior_event > historical[1].prior_event);
        assert!(restart_label(&historical, 2).contains("run 2"));
        assert_eq!(
            restart_candidates(&store, &session, &workflow).unwrap(),
            vec![
                RestartCandidate {
                    step: "plan".into(),
                    prior_event: init
                },
                RestartCandidate {
                    step: "implement".into(),
                    prior_event: plan_done
                },
            ]
        );
        let mut inherited = session.clone();
        store.fork(&mut inherited, plan_done, "plan").unwrap();
        let visible = historical_restart_candidates(&store, &inherited, &workflow).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].step, "plan");
    }
    #[test]
    fn branch_comparison_uses_captured_hashes_and_rejects_other_sessions() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "Goal", "work").unwrap();
        std::fs::write(root.path().join("file.txt"), "before").unwrap();
        let (checkpoint, _) =
            crate::workspace::record(root.path(), &store, &s, "work", "before").unwrap();
        let left = s.active_branch.clone();
        store.fork(&mut s, checkpoint, "work").unwrap();
        std::fs::write(root.path().join("file.txt"), "after").unwrap();
        crate::workspace::record(root.path(), &store, &s, "work", "after").unwrap();
        let report = compare_branches(&store, &s, &left, &s.active_branch).unwrap();
        assert!(report.contains("modified · file.txt"));
        assert_eq!(store.session(&s.id).unwrap().active_branch, s.active_branch);
        assert_eq!(
            std::fs::read_to_string(root.path().join("file.txt")).unwrap(),
            "after"
        );
        let other = store.create("demo", "Other", "work").unwrap();
        assert!(compare_branches(&store, &s, &left, &other.active_branch).is_err());
        let empty =
            compare_branches(&store, &other, &other.active_branch, &other.active_branch).unwrap();
        assert!(empty.contains("Comparison unavailable"));
        assert!(!empty.contains("No hash differences"));
    }
}
