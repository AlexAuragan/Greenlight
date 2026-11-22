//! Guarded restart selection, preview and confirmation.
use super::*;

impl App {
    pub(super) fn prepare_manager_restart(
        &mut self,
        root: &Path,
        store: &Store,
        message: &str,
    ) -> Result<()> {
        if self.runner.is_some() {
            bail!("Stop the worker before reviewing a restart");
        }
        let session = self.session().context("No selected session")?.clone();
        let review = store.prepare_manager_restart(root, &session.id, message)?;
        self.restart_preview = format!(
            "Confirm restart to {} with this original instruction:\n{}\n\n{}",
            review.route.step,
            review.route.original,
            review.plan.preview_text()
        );
        self.restart_branch = Some(session.active_branch.clone());
        self.restart_route = Some((review.route, review.fingerprint));
        self.restart_choices = vec![review.candidate];
        self.restart_cursor = 0;
        self.restart_plan = Some(review.plan);
        self.result_open = false;
        self.clear_diagnostic();
        self.detail_scroll = 0;
        self.notice =
            "Review restore and original instruction. y confirms; n/Esc cancels without delivery."
                .into();
        Ok(())
    }
    pub(super) fn open_restart(&mut self, root: &Path, store: &Store) {
        self.result_open = false;
        self.restart_route = None;
        if self.runner.is_some() {
            self.notice = "Cannot restart while an agent is running.".into();
            return;
        }
        let Some(session) = self.session() else {
            return;
        };
        match tui_actions::candidates_for_session(root, store, session) {
            Ok(choices) if !choices.is_empty() => {
                self.restart_cursor = choices
                    .iter()
                    .position(|candidate| candidate.step == session.current_step)
                    .unwrap_or(choices.len() - 1);
                self.restart_choices = choices;
                self.notice = "Select the step to restart. Enter previews; Esc cancels.".into();
            }
            Ok(_) => self.notice = "No recorded step attempts to restart.".into(),
            Err(error) => self.notice = format!("Restart unavailable: {error:#}"),
        }
    }
    pub(super) fn prepare_restart(&mut self, root: &Path, store: &Store) {
        let Some(candidate) = self.restart_choices.get(self.restart_cursor).cloned() else {
            return;
        };
        let Some(session) = self.session() else {
            return;
        };
        let branch = session.active_branch.clone();
        match RestorePlan::prepare(root, store, session, candidate.prior_event) {
            Ok(plan) => {
                self.restart_preview = format!(
                    "Restart '{}' from event #{} (the previous checkpoint).\n\n{}",
                    candidate.step,
                    candidate.prior_event,
                    plan.preview_text(),
                );
                self.restart_branch = Some(branch);
                self.restart_plan = Some(plan);
                self.detail_scroll = 0;
                self.notice = "Review the entire diff, then y confirms / n cancels.".into();
            }
            Err(error) => self.notice = format!("Restart refused: {error:#}"),
        }
    }
    pub(super) fn confirm_restart(&mut self, root: &Path, store: &Store) {
        let Some(candidate) = self.restart_choices.get(self.restart_cursor).cloned() else {
            return;
        };
        let Some(selected) = self.session() else {
            return;
        };
        let mut state = match store.session(&selected.id) {
            Ok(session) => session,
            Err(error) => {
                self.notice = format!("Restart cancelled: {error:#}");
                return;
            }
        };
        if state.status == "running"
            || self.restart_branch.as_deref() != Some(state.active_branch.as_str())
        {
            self.notice = "Restart cancelled: session changed since preview.".into();
            self.restart_plan = None;
            self.restart_choices.clear();
            return;
        }
        let Some(plan) = self.restart_plan.take() else {
            return;
        };
        if let Some((route, fingerprint)) = self.restart_route.take() {
            let review = crate::manager_delivery::RouteRestart {
                route,
                candidate,
                plan,
                fingerprint,
            };
            match store.confirm_manager_restart(root, &mut state, review, false) {
                Ok(route) => {
                    self.requested_session = Some(state.id.clone());
                    self.refresh(store).ok();
                    self.compose.last_instruction = Some((
                        (
                            Some(state.id.clone()),
                            Some(state.active_branch),
                            compose::Mode::Step,
                        ),
                        route.original,
                    ));
                    self.runner = Some(tui_actions::resume_route(
                        root.to_path_buf(),
                        state.id,
                        route.message_id,
                    ));
                    self.notice="Restart confirmed; original instruction reserved for the target. Starting the new attempt with normal approvals and verification.".into();
                }
                Err(error) => self.raise_diagnostic(format!("Guided restart failed: {error:#}")),
            }
            self.restart_choices.clear();
            self.restart_branch = None;
            self.detail_scroll = 0;
            return;
        }
        match store.restore_and_fork(
            root,
            &mut state,
            plan,
            candidate.prior_event,
            &candidate.step,
            false,
        ) {
            Ok(()) => {
                self.notice = format!(
                    "Forked from event #{} to '{}'. Type an instruction or /resume.",
                    candidate.prior_event, candidate.step
                )
            }
            Err(error) => self.raise_diagnostic(format!(
                "Restart failed: {error:#}. Inspect workspace and history before retrying."
            )),
        }
        self.restart_choices.clear();
        self.restart_branch = None;
        self.focus_composer();
        self.detail_scroll = 0;
    }
}

pub(super) fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.restart_plan.is_some() {
        draw_scrolled(
            frame,
            app,
            Paragraph::new(app.restart_preview.clone())
                .wrap(Wrap { trim: false })
                .block(border(" Restart preview — y confirm / n cancel ", true)),
            area,
        );
    } else if !app.restart_choices.is_empty() {
        let choices = app.restart_choices.iter().map(|candidate| {
            ListItem::new(format!(
                "{}  — restore before event #{}",
                candidate.step, candidate.prior_event
            ))
        });
        let choices_widget = List::new(choices)
            .block(border(
                " Restart from step — Enter to preview / Esc cancel ",
                true,
            ))
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
        let mut state = ListState::default();
        state.select(Some(app.restart_cursor));
        frame.render_stateful_widget(choices_widget, area, &mut state);
    }
}
