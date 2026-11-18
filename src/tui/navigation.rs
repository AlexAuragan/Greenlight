//! Candidate browsing is separate from the opened workspace and its drafts.
use super::*;
#[derive(Default)]
pub(super) struct Navigation {
    pub candidates: Vec<Session>,
    pub index: usize,
    pub selected: Option<String>,
    pub opened: Option<String>,
    pub error: Option<String>,
    pub load_error: Option<String>,
    pub missing: Option<String>,
    pub setup: Option<String>,
    query: String,
    before: Option<Return>,
}
struct Return {
    focus: Focus,
    scroll: u16,
    scope: Option<(String, String)>,
    result: Option<(String, String, i64)>,
}
impl Navigation {
    pub fn candidate(&self) -> Option<&Session> {
        self.candidates
            .get(self.index)
            .filter(|s| Some(&s.id) == self.selected.as_ref())
    }
    pub fn refresh(
        &mut self,
        all: &[Session],
        query: &str,
        activity: &crate::session_picker::Activity,
    ) {
        self.candidates = all.to_vec();
        crate::session_picker::filter_and_sort(&mut self.candidates, query, activity);
        let query_changed = self.query != query;
        self.query = query.into();
        let desired = self.selected.as_ref().or(self.opened.as_ref());
        self.index = desired
            .and_then(|id| self.candidates.iter().position(|s| &s.id == id))
            .unwrap_or_else(|| {
                if query_changed || self.selected.is_none() {
                    0
                } else {
                    usize::MAX
                }
            });
        if let Some(s) = self.candidates.get(self.index) {
            self.selected = Some(s.id.clone());
        }
    }
    pub fn select(&mut self, delta: isize) {
        self.index = if self.index >= self.candidates.len() {
            if delta < 0 {
                self.candidates.len().saturating_sub(1)
            } else {
                0
            }
        } else {
            move_index(self.index, delta, self.candidates.len())
        };
        self.selected = self.candidates.get(self.index).map(|s| s.id.clone());
    }
}
impl App {
    pub(super) fn open_navigator(&mut self, editing: bool) {
        if !self.shell.sessions {
            self.remember_reading();
            self.navigation.before = Some(Return {
                focus: self.focus,
                scroll: self.detail_scroll,
                scope: self
                    .session()
                    .map(|s| (s.id.clone(), s.active_branch.clone())),
                result: self.result_key.clone(),
            });
            self.navigation.selected = self.session().map(|s| s.id.clone());
            self.navigation
                .refresh(&self.sessions, &self.session_query, &self.session_activity);
        }
        self.shell.sessions = true;
        self.searching = editing;
        self.focus = Focus::Sessions;
        self.shell.actions_focus = false;
        if editing {
            self.session_editor.reset();
        }
    }
    pub(super) fn close_navigator(&mut self) {
        self.shell.sessions = false;
        self.searching = false;
        self.shell.actions_focus = false;
        if let Some(before) = self.navigation.before.take() {
            if before.scope
                == self
                    .session()
                    .map(|s| (s.id.clone(), s.active_branch.clone()))
                && before.result == self.result_key
            {
                self.focus = before.focus;
                self.detail_scroll = before.scroll;
                return;
            }
        }
        self.focus = self.main_focus();
        self.restore_reading();
    }
    pub(super) fn open_candidate(&mut self, store: &Store) -> Result<()> {
        let Some(id) = self.navigation.candidate().map(|s| s.id.clone()) else {
            self.notice = "No available session selected. Choose a row or clear the search.".into();
            return Ok(());
        };
        // Recheck identity before changing any workspace, even after a stale list click.
        if let Err(error) = store.session(&id) {
            self.notice = format!("Selected session unavailable; workspace unchanged: {error:#}");
            self.refresh(store)?;
            return Ok(());
        }
        self.shell.sessions = false;
        self.searching = false;
        self.shell.actions_focus = false;
        self.navigation.before = None;
        self.requested_session = Some(id);
        self.focus = self.main_focus();
        self.refresh(store)?;
        self.notice = if self.navigation.load_error.is_some(){"Selected session data is unavailable; no different run was opened.".into()}else{"Opened selected session. No workflow was started or resumed.".into()};
        Ok(())
    }
}
pub(super) fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let block = border(
        " Sessions · browse, then Open ",
        app.focus == Focus::Sessions,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let compact = area.height < 12;
    let rows = if compact { 2 } else { 3 };
    let panels = Layout::vertical([
        Constraint::Length(if compact { 1 } else { 3 }),
        Constraint::Min(1),
        Constraint::Length(if compact { 0 } else { 2 }),
    ])
    .split(inner);
    let search = Block::bordered()
        .title(if app.searching {
            " Find session "
        } else {
            " Search · f edits "
        })
        .border_style(Style::default().fg(if app.searching {
            theme::FOCUS
        } else {
            theme::BORDER
        }));
    let input = if compact {
        panels[0]
    } else {
        search.inner(panels[0])
    };
    if !compact {
        frame.render_widget(search, panels[0]);
    }
    if !app.searching {
        app.shell.hit(input, shell::Action::NavSearch);
    }
    app.session_editor.draw(
        frame,
        &app.session_query,
        "Search goals, workflow or status…",
        input,
        app.searching && !app.help && !app.shell.actions_focus,
    );
    if let Some(error) = &app.navigation.error {
        frame.render_widget(
            Paragraph::new(safe_display(error))
                .style(Style::default().fg(theme::ERROR))
                .wrap(Wrap { trim: false }),
            panels[1],
        );
    } else if app.navigation.candidates.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.session_total == 0 {
                "No sessions in this project."
            } else {
                "No sessions match this search.\nClear search to browse all sessions."
            })
            .wrap(Wrap { trim: false }),
            panels[1],
        );
    } else {
        let items = app
            .navigation
            .candidates
            .iter()
            .map(|s| {
                let color = match s.status.as_str() {
                    "blocked" => theme::ATTENTION,
                    "running" => theme::FOCUS,
                    "completed" => theme::GOOD,
                    _ => theme::MUTED,
                };
                let opened = if app.session().is_some_and(|active| active.id == s.id) {
                    " · open"
                } else {
                    ""
                };
                let recent = app
                    .session_activity
                    .get(&s.id)
                    .map(|(_, time)| format!("Last: {time} UTC"))
                    .unwrap_or_else(|| "Activity time not recorded".into());
                let mut lines = vec![
                    Line::raw(super::view::fit(
                        &s.goal,
                        panels[1].width.saturating_sub(2) as usize,
                    )),
                    Line::styled(
                        super::view::fit(
                            &format!("{} · {}{opened}", s.status, s.current_step),
                            panels[1].width.saturating_sub(2) as usize,
                        ),
                        Style::default().fg(color),
                    ),
                ];
                if !compact {
                    lines.push(Line::styled(
                        super::view::fit(&recent, panels[1].width.saturating_sub(2) as usize),
                        Style::default().fg(theme::MUTED),
                    ));
                }
                ListItem::new(lines)
            })
            .collect::<Vec<_>>();
        let mut selected = ListState::default()
            .with_selected(app.navigation.candidate().map(|_| app.navigation.index));
        frame.render_stateful_widget(
            List::new(items).highlight_symbol("› ").highlight_style(
                Style::default()
                    .fg(if app.shell.actions_focus {
                        theme::TEXT
                    } else {
                        theme::FOCUS
                    })
                    .bg(theme::PANEL),
            ),
            panels[1],
            &mut selected,
        );
        for index in selected.offset()..app.navigation.candidates.len() {
            let y = panels[1].y + ((index - selected.offset()) * rows) as u16;
            if y + rows as u16 > panels[1].bottom() {
                break;
            }
            app.shell.hit(
                ratatui::layout::Rect::new(panels[1].x, y, panels[1].width, rows as u16),
                shell::Action::Session(index),
            );
        }
    }
    let text = if app.navigation.candidate().is_none()
        && app.navigation.selected.is_some()
        && !app.navigation.candidates.is_empty()
    {
        "Selected candidate unavailable; choose another row.".into()
    } else {
        format!(
            "{} of {} sessions\nBrowse only · Open switches workspace.",
            app.navigation.candidates.len(),
            app.session_total
        )
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(theme::MUTED))
            .wrap(Wrap { trim: false }),
        panels[2],
    );
}
