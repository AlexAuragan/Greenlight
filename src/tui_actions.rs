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

pub struct Runner {
    pub prompts: Receiver<ApprovalPrompt>,
    pub finished: Receiver<Result<(), String>>,
    pub answers: Sender<String>,
}

pub fn resume(root: PathBuf, session_id: String, instruction: String) -> Runner {
    let (prompt_tx, prompts) = mpsc::channel();
    let (answers, answer_rx) = mpsc::channel();
    let (finished_tx, finished) = mpsc::channel();

    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut engine = Engine::open(&root)?;
            engine.tools.use_tui(prompt_tx, answer_rx);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all().build()?;
            runtime.block_on(engine.run(&session_id, Some(&instruction), None))
        })();
        let _ = finished_tx.send(result.map_err(|error| format!("{error:#}")));
    });

    Runner { prompts, finished, answers }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestartCandidate {
    pub step: String,
    /// Restore to this event, before the first attempted execution of step.
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
        let Some(start) = history.iter().position(|(_, event_step, kind, _)| {
            event_step == step && kind == "step.started"
        }) else {
            continue;
        };
        // session.started is the earliest safe checkpoint for the first step.
        let prior_event = if start == 0 {
            history[0].0
        } else {
            history[start - 1].0
        };
        candidates.push(RestartCandidate { step: step.clone(), prior_event });
    }
    Ok(candidates)
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
    restart_candidates(store, session, &workflow)
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
        store.event(&session, Some("plan"), "step.started", "").unwrap();
        let plan_done = store.event(&session, Some("plan"), "step.completed", "").unwrap();
        store.event(&session, Some("implement"), "step.started", "").unwrap();
        store.event(&session, Some("implement"), "step.started", "retry").unwrap();
        let workflow = LoadedWorkflow {
            definition: crate::config::Workflow {
                id: "example".into(), title: "Example".into(), start: "plan".into(),
                steps: vec!["plan".into(), "implement".into()],
                defaults: crate::config::StepDefaults::default(),
            },
            steps: std::collections::HashMap::new(),
            folder: root.path().to_path_buf(),
        };
        assert_eq!(restart_candidates(&store, &session, &workflow).unwrap(), vec![
            RestartCandidate { step: "plan".into(), prior_event: init },
            RestartCandidate { step: "implement".into(), prior_event: plan_done },
        ]);
    }
}
