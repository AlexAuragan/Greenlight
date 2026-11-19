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
    pub live: Option<crate::provider::stream::Preview>,
    pub cancel_retry: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub session_id: Option<String>,
    pub prompts: Receiver<ApprovalPrompt>,
    pub finished: Receiver<Result<(), String>>,
    pub answers: Sender<String>,
}

pub fn resume(root: PathBuf, session_id: String, instruction: String) -> Runner {
    let (prompt_tx, prompts) = mpsc::channel();
    let (answers, answer_rx) = mpsc::channel();
    let (finished_tx, finished) = mpsc::channel();
    let owner_session = session_id.clone();
    let cancel_retry = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = cancel_retry.clone();

    let live = crate::provider::stream::Preview::default();
    let worker_live = live.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut engine = Engine::open(&root)?;
            engine.interactive_provider_retry = true;
            engine.provider.retry_cancel = worker_cancel;
            worker_live.lock().unwrap().connection = Some(engine.provider.connection_info());
            engine.provider.live = Some(worker_live);
            engine.tools.use_tui(prompt_tx, answer_rx);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(engine.run(&session_id, Some(&instruction), None))
        })();
        let _ = finished_tx.send(result.map_err(|error| format!("{error:#}")));
    });

    Runner {
        live: Some(live),
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
    }
}
