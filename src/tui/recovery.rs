//! Guarded restart selection, preview and confirmation.
use super::*;

impl App {
    pub(super) fn open_restart(&mut self, root: &Path, store: &Store) {
        self.result_open = false;
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
        if let Err(error) = plan.apply_silent(root, store, &state) {
            self.raise_diagnostic(format!(
                "Restore failed: {error:#}. Inspect history before retrying."
            ));
            self.restart_choices.clear();
            return;
        }
        match store.fork(&mut state, candidate.prior_event, &candidate.step) {
            Ok(()) => {
                self.notice = format!(
                    "Forked from event #{} to '{}'. Type an instruction or /resume.",
                    candidate.prior_event, candidate.step
                )
            }
            Err(error) => self.raise_diagnostic(format!(
                "Restore applied, but fork failed: {error:#}. Inspect workspace and history."
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
