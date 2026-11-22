//! Independent manager inference; ordinary conversation routes through validated host controls.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub(super) struct ManagerJob {
    session: String,
    branch: String,
    finished: mpsc::Receiver<std::result::Result<i64, String>>,
    root: PathBuf,
    automatic: bool,
    summary: bool,
    cancel: Arc<AtomicBool>,
}
impl App {
    pub(super) fn start_manager(
        &mut self,
        root: &Path,
        store: &Store,
        text: String,
        automatic: bool,
    ) {
        if self.manager_job.is_some() {
            self.notice = "A background request is already running; your draft is retained.".into();
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
        let parent = if automatic || self.compose.mode == compose::Mode::Manager {
            self.compose.clarification.get(&self.compose.scope).copied()
        } else {
            None
        };
        let saved = match crate::manager::save_clarification(store, &session, &text, parent) {
            Ok(saved) => saved,
            Err(error) => {
                self.notice =
                    format!("Manager message could not be saved: {error:#}. Draft retained.");
                return;
            }
        };
        if automatic {
            if let Err(error)=store.event(&saved.session,None,"manager.routing-requested",&serde_json::json!({"message_id":saved.message_id,"note":"User submitted a Conversation message; host may accept a validated forward. Restore confirmation and permissions remain separate."}).to_string()) {
                self.notice=format!("Original saved but routing intent could not be recorded: {error:#}. No inference started; draft retained.");return;
            }
        }
        let artifacts = Vec::new();
        let branch = saved.session.active_branch.clone();
        let (sent, finished) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let job_root = root.to_path_buf();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let mut opened = false;
            let result = (|| -> Result<i64> {
                let mut engine = crate::engine::Engine::open(&root)?;
                opened = true;
                engine.provider.retry_cancel = worker_cancel;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(engine.ask_saved_manager(&saved, &artifacts))?;
                result_event(&engine.store, &saved.session, &saved.message_id)
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
            root: job_root,
            automatic,
            summary: false,
            finished,
            cancel,
        });
        self.compose.clarification.remove(&self.compose.scope);
        self.composer.clear();
        self.focus = self.main_focus();
        self.follow_activity = true;
        self.notice = if automatic {
            "Message saved. Manager is deciding whether to answer, clarify or route to a step."
        } else {
            "Question saved. Manager is answering (read-only)."
        }
        .into();
    }
    pub(super) fn summary_running(&self) -> bool {
        self.manager_job.as_ref().is_some_and(|j| j.summary)
    }
    pub(super) fn start_summary(&mut self, root: &Path) -> Result<()> {
        if self.manager_job.is_some() {
            bail!("A background request is already running; wait or stop it first");
        }
        let session = self.session().context("Select a session")?.clone();
        let scope = session.clone();
        let job_root = root.to_path_buf();
        let root = root.to_path_buf();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sent, finished) = mpsc::channel();
        // The coding runner remains independent and retains all its own gates.
        std::thread::spawn(move || {
            let result = (|| -> Result<i64> {
                let mut engine = crate::engine::Engine::open(&root)?;
                engine.provider.retry_cancel = worker_cancel;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let summary =
                    runtime.block_on(engine.summarize(&scope.id, Some(&scope.active_branch)))?;
                engine
                    .store
                    .history(&scope)?
                    .iter()
                    .rev()
                    .find(|e| {
                        e.2 == "summary.result"
                            && serde_json::from_str::<crate::summarizer::SavedSummary>(&e.3)
                                .is_ok_and(|v| {
                                    v.snapshot.snapshot_id == summary.snapshot.snapshot_id
                                })
                    })
                    .map(|e| e.0)
                    .context("Summary result was not persisted")
            })();
            let _ = sent.send(result.map_err(|e| format!("{e:#}")));
        });
        self.manager_job = Some(ManagerJob {
            session: session.id,
            branch: session.active_branch,
            root: job_root,
            automatic: false,
            summary: true,
            finished,
            cancel,
        });
        self.notice="Generating an optional summary in the background (provider usage is recorded). Workflow continues independently.".into();
        Ok(())
    }
    pub(super) fn retry_saved_manager(&mut self, root: &Path, store: &Store) {
        if self.manager_job.is_some() {
            self.notice = "A manager request is already active.".into();
            return;
        }
        let Some(session) = self.session().cloned() else {
            return;
        };
        match store.pending_continuation(&session, true) {
            Ok(Some(_)) => {}
            Ok(None) => {
                self.notice = "No saved manager request.".into();
                return;
            }
            Err(error) => {
                self.notice = format!("Saved manager request unavailable: {error:#}");
                return;
            }
        }
        let job_root = root.to_path_buf();
        let root = root.to_path_buf();
        let id = session.id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sent, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> Result<i64> {
                let mut engine = crate::engine::Engine::open(&root)?;
                engine.provider.retry_cancel = worker_cancel;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let reply = runtime.block_on(engine.resume_manager_saved(&id))?;
                result_event(
                    &engine.store,
                    &engine.store.session(&id)?,
                    reply.decision.message_id(),
                )
            })();
            let _ = sent.send(result.map_err(|error| format!("{error:#}")));
        });
        self.manager_job = Some(ManagerJob {
            session: session.id,
            branch: session.active_branch,
            root: job_root,
            automatic: false,
            summary: false,
            cancel,
            finished,
        });
        self.notice =
            "Retrying the exact saved manager request; any forward will require explicit acceptance."
                .into();
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
        let root = job.root.clone();
        let automatic = job.automatic && !job.cancel.load(Ordering::SeqCst);
        let scope = (job.session.clone(), job.branch.clone());
        let summary = job.summary;
        self.manager_job = None;
        let handled = (|| -> Result<()> {
            let event = result.map_err(anyhow::Error::msg)?;
            let store = Store::open(&root)?;
            let state = store.session(&scope.0)?;
            if !selected || state.active_branch != scope.1 {
                self.notice="Background result saved for its original session/branch; inspect its recorded history. No workflow execution started.".into();
                return Ok(());
            }
            let may_present = !self.reading_overlay() && self.focus != Focus::Compose;
            let history = store.history(&state)?;
            let raw = &history
                .iter()
                .find(|e| {
                    e.0 == event
                        && e.2
                            == if summary {
                                "summary.result"
                            } else {
                                "manager.result"
                            }
                })
                .context("Manager result unavailable")?
                .3;
            if summary {
                let saved: crate::summarizer::SavedSummary = serde_json::from_str(raw)?;
                if may_present {
                    self.context_detail = Some(saved.display());
                    self.detail_scroll = 0;
                }
                self.notice="Summary saved for its captured evidence. More → Latest summary reopens it; workflow state is unchanged.".into();
                return Ok(());
            }
            let reply: crate::manager::Reply = serde_json::from_str(raw)?;
            self.refresh(&store)?;
            if automatic && matches!(reply.decision, crate::manager::Decision::Forward { .. }) {
                self.deliver_manager_result(&root, &store, event)?;
            } else {
                self.notice=if matches!(reply.decision,crate::manager::Decision::Clarify{..}) {"Manager needs clarification. Open Manager replies and choose Reply to clarification."} else {"Manager reply saved. Any proposed route still requires explicit acceptance."}.into();
                if may_present {
                    self.switch_view(View::Conversation);
                    self.result_open = false;
                    if let Some(index) = self.events.iter().position(|e| e.id == event) {
                        self.event_index = index;
                        self.inspector_open = true;
                        self.focus = Focus::Detail;
                        self.detail_scroll = 0;
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = handled {
            self.notice=format!("Background request/result unavailable: {error:#}. Inspect recorded history and provider diagnostics; no fallback execution was attempted.");
        }
    }
    pub(super) fn deliver_manager_result(
        &mut self,
        root: &Path,
        store: &Store,
        event: i64,
    ) -> Result<()> {
        let session = self.session().context("No selected session")?.clone();
        let accepted = store.accept_manager_route(root, &session.id, event)?;
        let route = accepted.route;
        self.notice = match route.state.as_str() {
            "queued" if route.attempt.is_some() => format!(
                "Instruction queued for {}'s next coding request; current work may finish first.",
                route.step
            ),
            "queued" => format!(
                "Instruction queued for the next visit to {} on this branch.",
                route.step
            ),
            "restart_proposed" => format!(
                "Restart proposed for {}. Review restart to continue; no files restored.",
                route.step
            ),
            "included_in_request" => format!(
                "Instruction already included for {}; no new attempt started.",
                route.step
            ),
            _ => format!(
                "Instruction status: {}. Inspect Instruction delivery for details.",
                route.state
            ),
        };
        self.refresh(store)?;
        if route.state == "restart_proposed"
            && !self.reading_overlay()
            && self.focus != Focus::Compose
        {
            if let Err(error) = self.prepare_manager_restart(root, store, &route.message_id) {
                self.notice = format!(
                    "Restart proposed; stop the worker before reviewing restoration: {error:#}"
                );
            }
        } else if accepted.newly_accepted && accepted.start_current && self.runner.is_none() {
            self.compose.last_instruction = Some((
                (
                    Some(session.id.clone()),
                    Some(session.active_branch),
                    compose::Mode::Step,
                ),
                route.original,
            ));
            self.runner = Some(tui_actions::resume_route(
                root.to_path_buf(),
                session.id,
                route.message_id,
            ));
            self.notice="Starting the routed step with your saved original. Permissions and verification remain separate.".into();
        }
        Ok(())
    }
    pub(super) fn cancel_manager(&mut self) {
        if let Some(job) = &self.manager_job {
            job.cancel.store(true, Ordering::SeqCst);
            self.notice =
                "Stopping background request… Remote processing/billing may be uncertain.".into();
        } else {
            self.notice = "No background request is active.".into();
        }
    }
}

fn result_event(store: &Store, session: &crate::store::Session, message: &str) -> Result<i64> {
    store
        .history(session)?
        .iter()
        .rev()
        .find(|e| {
            e.2 == "manager.result"
                && serde_json::from_str::<crate::manager::Reply>(&e.3)
                    .is_ok_and(|r| r.decision.message_id() == message)
        })
        .map(|e| e.0)
        .context("Manager result was not persisted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::{Decision, Reply};
    use serde_json::json;
    fn fixture(visited: bool) -> (tempfile::TempDir, Store, Session, App, i64) {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        std::fs::write(
            root.path().join(".greenlight/config.yaml"),
            "provider:\n  model: fixture\nmanager:\n  enabled: true\n",
        )
        .unwrap();
        let folder = root.path().join(".greenlight/workflows/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work, later]\n",
        )
        .unwrap();
        for step in ["work", "later"] {
            std::fs::write(
                folder.join(format!("{step}.md")),
                format!("---\nid: {step}\nverification:\n  type: none\n---\nWork\n"),
            )
            .unwrap();
        }
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "Goal", "work").unwrap();
        if visited {
            store
                .event(
                    &session,
                    Some("later"),
                    "step.started",
                    "Legacy previous run",
                )
                .unwrap();
        }
        let saved =
            crate::manager::save_message(&store, &session.id, "  Use threads — é\n ").unwrap();
        let snapshot =
            crate::manager::capture(root.path(), &store, &session.id, &saved.message_id, &[])
                .unwrap();
        store
            .event(
                &session,
                None,
                "manager.snapshot",
                &serde_json::to_string(&snapshot).unwrap(),
            )
            .unwrap();
        let decision=Decision::parse(&json!({"schema_version":1,"message_id":saved.message_id,"snapshot_id":snapshot.snapshot_id,"action":"forward","step_id":"later","content":"Route explanation","citations":["workflow:demo"]}).to_string(),&snapshot).unwrap();
        let reply = Reply {
            decision,
            status: "forward_proposed".into(),
            snapshot_changed: false,
            captured_at: snapshot.captured_at,
            sources: snapshot.sources,
        };
        let event = store
            .event(
                &session,
                None,
                "manager.result",
                &serde_json::to_string(&reply).unwrap(),
            )
            .unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        let (sent, finished) = mpsc::channel();
        sent.send(Ok(event)).unwrap();
        app.manager_job = Some(ManagerJob {
            session: session.id.clone(),
            branch: session.active_branch.clone(),
            root: root.path().to_path_buf(),
            automatic: true,
            summary: false,
            finished,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        (root, store, session, app, event)
    }
    #[test]
    fn automatic_forward_respects_cancellation_state_changes_and_session_navigation() {
        for scenario in ["cancel", "stale", "other-session", "error"] {
            let (_root, store, session, mut app, _event) = fixture(false);
            match scenario {
                "cancel" => app.cancel_manager(),
                "error" => {
                    let (sent, finished) = mpsc::channel();
                    sent.send(Err("Fixture provider failure".into())).unwrap();
                    app.manager_job.as_mut().unwrap().finished = finished;
                }
                "stale" => {
                    store
                        .event(
                            &session,
                            Some("work"),
                            "human.approved",
                            "Changed since inference",
                        )
                        .unwrap();
                }
                _ => {
                    let other = store.create("demo", "Other", "work").unwrap();
                    app.requested_session = Some(other.id);
                    app.refresh(&store).unwrap();
                }
            }
            app.poll_manager();
            assert!(
                store.manager_routes(&session).unwrap().is_empty(),
                "{scenario}"
            );
            assert!(app.runner.is_none());
            assert_eq!(store.session(&session.id).unwrap().status, "ready");
        }
    }
    #[test]
    fn filtered_future_route_queues_without_stealing_an_approval_or_a_new_draft() {
        for approval in [false, true] {
            let (_root, store, session, mut app, _event) = fixture(false);
            app.activity_query = "no matching result".into();
            app.refresh(&store).unwrap();
            if approval {
                app.approval = Some(ApprovalPrompt {
                    review: None,
                    command: None,
                    content: "Human gate remains pending".into(),
                    mode: ApprovalMode::Human,
                });
            } else {
                app.focus = Focus::Compose;
                app.composer = "Unsent next message".into();
            }
            app.poll_manager();
            let routes = store.manager_routes(&session).unwrap();
            assert_eq!(routes.len(), 1, "{}", app.notice);
            assert_eq!(routes[0].state, "queued");
            assert_eq!(routes[0].step, "later");
            assert!(routes[0].attempt.is_none());
            assert!(app.runner.is_none());
            if approval {
                assert!(app.approval.is_some());
            } else {
                assert_eq!(app.focus, Focus::Compose);
                assert_eq!(app.composer, "Unsent next message");
            }
        }
    }
    #[test]
    fn automatic_restart_requires_review_and_does_not_restore_or_fork() {
        let (_root, store, session, mut app, _event) = fixture(true);
        let before = store.session(&session.id).unwrap();
        app.poll_manager();
        let after = store.session(&session.id).unwrap();
        assert_eq!(before.active_branch, after.active_branch);
        assert_eq!(before.current_step, after.current_step);
        assert!(app.runner.is_none());
        assert_eq!(
            store.manager_routes(&after).unwrap()[0].state,
            "restart_proposed"
        );
        assert!(!store.history(&after).unwrap().iter().any(|e| matches!(
            e.2.as_str(),
            "manager.restart-authorized" | "workspace.restore-completed" | "branch.forked"
        )));
    }
}
