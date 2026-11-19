//! Manager inference is independent of the coding worker and cannot execute a route.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub(super) struct ManagerJob {
    session: String,
    branch: String,
    finished: mpsc::Receiver<std::result::Result<(), String>>,
    cancel: Arc<AtomicBool>,
}
impl App {
    pub(super) fn start_manager(&mut self, root: &Path, store: &Store, text: String) {
        if self.manager_job.is_some() {
            self.notice = "A manager request is already running; your draft is retained.".into();
            return;
        }
        let Some(session) = self.session().map(|s| s.id.clone()) else {
            self.notice = "Choose a session before asking the manager.".into();
            return;
        };
        if text.trim().is_empty() || text.len() > crate::manager::MESSAGE_BYTES {
            self.notice =
                "Manager messages must be nonempty and at most 12 KiB; draft retained.".into();
            return;
        }
        let saved = match crate::manager::save_message(store, &session, &text) {
            Ok(saved) => saved,
            Err(error) => {
                self.notice =
                    format!("Manager message could not be saved: {error:#}. Draft retained.");
                return;
            }
        };
        let artifacts = Vec::new();
        let branch = saved.session.active_branch.clone();
        let (sent, finished) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let mut opened = false;
            let result = (|| -> Result<()> {
                let mut engine = crate::engine::Engine::open(&root)?;
                opened = true;
                engine.provider.retry_cancel = worker_cancel;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(engine.ask_saved_manager(&saved, &artifacts))?;
                Ok(())
            })();
            if !opened {
                if let Ok(store) = Store::open(&root) {
                    let _=store.event(&saved.session,None,"manager.failed",&serde_json::json!({"message_id":saved.message_id,"status":"unresolved","message":"Manager configuration/worker unavailable; original message retained."}).to_string());
                }
            }
            let _ = sent.send(result.map_err(|e| format!("{e:#}")));
        });
        self.manager_job = Some(ManagerJob {
            session,
            branch,
            finished,
            cancel,
        });
        self.composer.clear();
        self.focus = self.main_focus();
        self.follow_activity = true;
        self.notice = "Question saved. Manager is answering (read-only).".into();
    }
    pub(super) fn poll_manager(&mut self) {
        let Some(job) = self.manager_job.as_ref() else {
            return;
        };
        let result = match job.finished.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(
                "Manager worker disconnected; inspect saved history/usage before retrying.".into(),
            ),
            Ok(result) => result,
        };
        let selected = self
            .session()
            .is_some_and(|s| s.id == job.session && s.active_branch == job.branch);
        self.notice=match result {
            Ok(()) if !selected=>"Manager answer saved for its original session/branch. Review that history; no instructions delivered.".into(),
            Ok(())=>"Manager answer saved. More → Manager replies opens the recorded exchange.".into(),
            Err(error)=>format!("Manager request for {}, branch {} failed: {error}. No route executed.",job.session,job.branch),
        };
        self.manager_job = None;
    }
    pub(super) fn cancel_manager(&mut self) {
        if let Some(job) = &self.manager_job {
            job.cancel.store(true, Ordering::SeqCst);
            self.notice =
                "Stopping manager request… Remote processing/billing may be uncertain.".into();
        } else {
            self.notice = "No manager request is active.".into();
        }
    }
}
