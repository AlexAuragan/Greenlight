//! Read-only live session viewer. The engine remains the sole writer of
//! session history and the sole owner of human approval prompts.
use crate::{
    changes,
    restore::{self, RestorePlan},
    store::{Session, Store},
    tui_actions::{self, RestartCandidate, Runner},
};
use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    time::Duration,
};

#[derive(Clone, Debug)]
struct EventRow {
    id: i64,
    step: String,
    kind: String,
    data: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Sessions,
    Events,
    Compose,
}

struct App {
    sessions: Vec<Session>,
    session_index: usize,
    events: Vec<EventRow>,
    event_index: usize,
    focus: Focus,
    show_all: bool,
    detail_scroll: u16,
    diff: Option<(i64, String)>,
    requested_session: Option<String>,
    composer: String,
    notice: String,
    runner: Option<Runner>,
    approval: Option<String>,
    approval_choice: char,
    restart_choices: Vec<RestartCandidate>,
    restart_cursor: usize,
    restart_plan: Option<RestorePlan>,
    restart_preview: String,
    restart_branch: Option<String>,
}

impl App {
    fn new(initial_session: Option<&str>) -> Self {
        Self {
            sessions: Vec::new(),
            session_index: 0,
            events: Vec::new(),
            event_index: 0,
            focus: if initial_session.is_some() {
                Focus::Events
            } else {
                Focus::Sessions
            },
            show_all: false,
            detail_scroll: 0,
            diff: None,
            requested_session: initial_session.map(str::to_owned),
            composer: String::new(),
            notice: "Enter: compose  /restart: choose a step  Tab: navigate".into(),
            runner: None,
            approval: None,
            approval_choice: 'n',
            restart_choices: Vec::new(),
            restart_cursor: 0,
            restart_plan: None,
            restart_preview: String::new(),
            restart_branch: None,
        }
    }

    fn session(&self) -> Option<&Session> {
        self.sessions.get(self.session_index)
    }

    fn event(&self) -> Option<&EventRow> {
        self.events.get(self.event_index)
    }

    fn refresh(&mut self, store: &Store) -> Result<()> {
        let old_session = self.session().map(|s| s.id.clone());
        let old_event = self.event().map(|e| e.id);
        let following_tail = self.events.len().checked_sub(1) == Some(self.event_index);

        self.sessions = store.list()?;
        let desired_session = self.requested_session.take().or(old_session.clone());
        if let Some(id) = desired_session {
            self.session_index = self
                .sessions
                .iter()
                .position(|s| s.id == id)
                .unwrap_or(0);
        } else {
            self.session_index = 0;
        }
        let changed_session = old_session.as_deref() != self.session().map(|s| s.id.as_str());
        if let Some(session) = self.session() {
            let history = store.history(session)?;
            let show_all = self.show_all;
            self.events = history
                .into_iter()
                .filter(|(_, _, kind, _)| show_all || !is_chatter(kind))
                .map(|(id, step, kind, data)| EventRow { id, step, kind, data })
                .collect();
        } else {
            self.events.clear();
        }
        if changed_session || following_tail {
            self.event_index = self.events.len().saturating_sub(1);
            self.detail_scroll = 0;
            self.diff = None;
        } else if let Some(id) = old_event {
            self.event_index = self
                .events
                .iter()
                .position(|event| event.id == id)
                .unwrap_or(self.events.len().saturating_sub(1));
        } else {
            self.event_index = self.events.len().saturating_sub(1);
        }
        if self.diff.as_ref().is_some_and(|(id, _)| self.event().map(|event| event.id) != Some(*id)) {
            self.diff = None;
        }
        Ok(())
    }

    fn navigate(&mut self, delta: isize) {
        match self.focus {
            Focus::Compose => return,
            Focus::Sessions => {
                let next = move_index(self.session_index, delta, self.sessions.len());
                if next != self.session_index {
                    self.session_index = next;
                    self.events.clear();
                    self.event_index = 0;
                    self.detail_scroll = 0;
                    self.diff = None;
                }
            }
            Focus::Events => {
                let next = move_index(self.event_index, delta, self.events.len());
                if next != self.event_index {
                    self.event_index = next;
                    self.detail_scroll = 0;
                    self.diff = None;
                }
            }
        }
    }

    fn select_end(&mut self, last: bool) {
        match self.focus {
            Focus::Compose => return,
            Focus::Sessions => {
                let next = if last { self.sessions.len().saturating_sub(1) } else { 0 };
                if next != self.session_index {
                    self.session_index = next;
                    self.events.clear();
                    self.event_index = 0;
                    self.detail_scroll = 0;
                    self.diff = None;
                }
            }
            Focus::Events => {
                self.event_index = if last { self.events.len().saturating_sub(1) } else { 0 };
                self.detail_scroll = 0;
                self.diff = None;
            }
        }
    }

    fn toggle_verbose(&mut self) {
        self.show_all = !self.show_all;
        self.diff = None;
        self.detail_scroll = 0;
    }

    fn toggle_diff(&mut self, store: &Store) -> Result<()> {
        if self.diff.is_some() {
            self.diff = None;
            self.detail_scroll = 0;
            return Ok(());
        }
        let event = match self.event() {
            Some(event) if matches!(event.kind.as_str(), "file.changed" | "workspace.restored") => event,
            _ => return Ok(()),
        };
        let session = self.session().context("no selected session")?;
        let edit = store
            .workspace_edits(session)?
            .into_iter()
            .find(|entry| entry.event_id == event.id)
            .context("edit snapshots missing for selected event")?;
        let mut output = changes::unified(
            &edit.path, edit.before.as_deref(), edit.after.as_deref().unwrap_or(""),
        );
        if edit.after.is_none() {
            output = output.replacen(
                &format!("+++ b/{}\n", edit.path),
                "+++ /dev/null\n",
                1,
            );
        }
        self.diff = Some((event.id, output));
        self.detail_scroll = 0;
        Ok(())
    }

    fn detail(&self) -> String {
        if let Some((id, diff)) = &self.diff {
            if self.event().is_some_and(|event| event.id == *id) {
                return diff.clone();
            }
        }
        match self.event() {
            Some(event) => format!(
                "Event #{}  ·  {}  ·  {}\n\n{}",
                event.id, event.step, event.kind, event.data,
            ),
            None => "No events. Start a workflow from another terminal.".to_owned(),
        }
    }

    fn tick(&mut self) {
        let Some(runner) = &self.runner else { return };
        if self.approval.is_none() {
            match runner.prompts.try_recv() {
                Ok(question) => {
                    self.approval = Some(question);
                    self.approval_choice = 'n';
                    self.detail_scroll = 0;
                }
                Err(TryRecvError::Disconnected | TryRecvError::Empty) => {}
            }
        }
        let completion = runner.finished.try_recv();
        if let Ok(result) = completion {
            self.notice = match result {
                Ok(()) => "Workflow stopped or completed. See its latest events.".into(),
                Err(error) => format!("Runner error: {error}"),
            };
            self.runner = None;
            self.approval = None;
            self.focus = Focus::Compose;
        }
    }

    fn submit(&mut self, root: &Path, store: &Store) {
        let instruction = self.composer.trim().to_string();
        if instruction.is_empty() {
            return;
        }
        if instruction == "/restart" {
            self.open_restart(root, store);
            return;
        }
        if instruction == "/events" {
            self.toggle_verbose();
            self.composer.clear();
            return;
        }
        if instruction == "/help" {
            self.notice = "Type a message to resume; /restart selects an earlier step; /events toggles protocol detail; Ctrl+J adds a newline.".into();
            self.composer.clear();
            return;
        }
        if instruction.starts_with('/') && instruction != "/resume" {
            self.notice = format!("Unknown command: {}", instruction.lines().next().unwrap_or(""));
            return;
        }
        if self.runner.is_some() {
            self.notice = "An agent is already running. Wait for it to finish.".into();
            return;
        }
        let Some(selected) = self.session() else {
            self.notice = "No session selected.".into();
            return;
        };
        let session = match store.session(&selected.id) {
            Ok(session) => session,
            Err(error) => { self.notice = format!("Cannot load session: {error:#}"); return; }
        };
        if !matches!(session.status.as_str(), "blocked" | "ready") {
            self.notice = format!("Session is {}; only ready or blocked sessions can resume.", session.status);
            return;
        }
        let input = if instruction == "/resume" { "" } else { &instruction };
        self.runner = Some(tui_actions::resume(
            root.to_path_buf(), session.id, input.to_owned(),
        ));
        self.composer.clear();
        self.notice = "Running agent. Incoming approvals will appear here.".into();
        self.focus = Focus::Events;
    }

    fn open_restart(&mut self, root: &Path, store: &Store) {
        if self.runner.is_some() {
            self.notice = "Cannot restart while an agent is running.".into();
            return;
        }
        let Some(session) = self.session() else { return };
        match tui_actions::candidates_for_session(root, store, session) {
            Ok(choices) if !choices.is_empty() => {
                self.restart_cursor = choices
                    .iter()
                    .position(|candidate| candidate.step == session.current_step)
                    .unwrap_or(choices.len() - 1);
                self.restart_choices = choices;
                self.composer.clear();
                self.notice = "Select the step to restart. Enter previews; Esc cancels.".into();
            }
            Ok(_) => self.notice = "No recorded step attempts to restart.".into(),
            Err(error) => self.notice = format!("Restart unavailable: {error:#}"),
        }
    }

    fn prepare_restart(&mut self, root: &Path, store: &Store) {
        let Some(candidate) = self.restart_choices.get(self.restart_cursor).cloned() else { return };
        let Some(session) = self.session() else { return };
        match RestorePlan::prepare(root, store, session, candidate.prior_event) {
            Ok(plan) => {
                self.restart_preview = format!(
                    "Restart '{}' from event #{} (the previous checkpoint).\n\n{}",
                    candidate.step, candidate.prior_event, plan.preview_text(),
                );
                self.restart_branch = Some(session.active_branch.clone());
                self.restart_plan = Some(plan);
                self.detail_scroll = 0;
                self.notice = "Review the entire diff, then y confirms / n cancels.".into();
            }
            Err(error) => self.notice = format!("Restart refused: {error:#}"),
        }
    }

    fn confirm_restart(&mut self, root: &Path, store: &Store) {
        let Some(candidate) = self.restart_choices.get(self.restart_cursor).cloned() else { return };
        let Some(selected) = self.session() else { return };
        let mut state = match store.session(&selected.id) {
            Ok(session) => session,
            Err(error) => { self.notice = format!("Restart cancelled: {error:#}"); return; }
        };
        if state.status == "running" || self.restart_branch.as_deref() != Some(state.active_branch.as_str()) {
            self.notice = "Restart cancelled: session changed since preview.".into();
            self.restart_plan = None;
            self.restart_choices.clear();
            return;
        }
        let Some(plan) = self.restart_plan.take() else { return };
        if let Err(error) = plan.apply_silent(root, store, &state) {
            self.notice = format!("Restore failed: {error:#}. Inspect history before retrying.");
            self.restart_choices.clear();
            return;
        }
        match store.fork(&mut state, candidate.prior_event, &candidate.step) {
            Ok(()) => self.notice = format!(
                "Forked from event #{} to '{}'. Type an instruction or /resume.",
                candidate.prior_event, candidate.step
            ),
            Err(error) => self.notice = format!(
                "Restore applied, but fork failed: {error:#}. Inspect workspace and history."
            ),
        }
        self.restart_choices.clear();
        self.restart_branch = None;
        self.composer.clear();
        self.focus = Focus::Compose;
        self.detail_scroll = 0;
    }

    fn on_key(&mut self, key: KeyEvent, root: &Path, store: &Store) -> Result<bool> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(false);
        }

        if self.approval.is_some() {
            match key.code {
                KeyCode::Char(choice) if "yapgn".contains(choice) => {
                    self.approval_choice = choice;
                }
                KeyCode::Enter | KeyCode::Esc => {
                    let answer = if key.code == KeyCode::Esc {
                        "n".to_string()
                    } else {
                        self.approval_choice.to_string()
                    };
                    if let Some(runner) = &self.runner {
                        runner.answers.send(answer).context("approval worker disconnected")?;
                    }
                    self.approval = None;
                    self.notice = "Decision sent to the engine.".into();
                }
                KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
                KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
                _ => {}
            }
            return Ok(false);
        }

        if self.restart_plan.is_some() {
            match key.code {
                KeyCode::Char('y') => self.confirm_restart(root, store),
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.restart_plan = None;
                    self.restart_choices.clear();
                    self.notice = "Restart cancelled; no files modified.".into();
                }
                KeyCode::PageDown | KeyCode::Char('j') => {
                    self.detail_scroll = self.detail_scroll.saturating_add(10);
                }
                KeyCode::PageUp | KeyCode::Char('k') => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(10);
                }
                _ => {}
            }
            return Ok(false);
        }

        if !self.restart_choices.is_empty() {
            match key.code {
                KeyCode::Esc => {
                    self.restart_choices.clear();
                    self.notice = "Restart selection cancelled.".into();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.restart_cursor = move_index(self.restart_cursor, 1, self.restart_choices.len());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.restart_cursor = move_index(self.restart_cursor, -1, self.restart_choices.len());
                }
                KeyCode::Enter => self.prepare_restart(root, store),
                _ => {}
            }
            return Ok(false);
        }

        if self.focus == Focus::Compose {
            match key.code {
                KeyCode::Tab => self.focus = Focus::Sessions,
                KeyCode::Esc => self.focus = Focus::Events,
                KeyCode::Enter => self.submit(root, store),
                KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.composer.push('\n');
                }
                KeyCode::Backspace => { self.composer.pop(); }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.composer.len() < 8_192 {
                        self.composer.push(c);
                    }
                }
                _ => {}
            }
            return Ok(false);
        }

        match key.code {
            KeyCode::Char('q') if self.runner.is_none() => return Ok(true),
            KeyCode::Char('q') => self.notice = "Agent running; wait for its result before exiting.".into(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Sessions => Focus::Events,
                    Focus::Events => Focus::Compose,
                    Focus::Compose => Focus::Sessions,
                };
            }
            KeyCode::Esc | KeyCode::Left => self.focus = Focus::Sessions,
            KeyCode::Right => self.focus = Focus::Events,
            KeyCode::Enter | KeyCode::Char('i') => self.focus = Focus::Compose,
            KeyCode::Char('/') => {
                self.focus = Focus::Compose;
                self.composer.push('/');
            }
            KeyCode::Char('j') | KeyCode::Down => self.navigate(1),
            KeyCode::Char('k') | KeyCode::Up => self.navigate(-1),
            KeyCode::Char('g') | KeyCode::Home => self.select_end(false),
            KeyCode::Char('G') | KeyCode::End => self.select_end(true),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            KeyCode::Char('v') => self.toggle_verbose(),
            KeyCode::Char('d') => self.toggle_diff(store)?,
            KeyCode::Char('r') => self.refresh(store)?,
            _ => {}
        }
        Ok(false)
    }

}

fn is_chatter(kind: &str) -> bool {
    matches!(
        kind,
        "step.definition" | "context.assembled" | "model.output" | "tool.request"
    )
}

fn move_index(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    current.saturating_add_signed(delta).min(len - 1)
}

fn border(title: &'static str, focused: bool) -> Block<'static> {
    let color = if focused { Color::Cyan } else { Color::DarkGray };
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(color))
}

fn conversational_label(kind: &str) -> &str {
    match kind {
        "user.instruction" => "You",
        "step.started" => "Step started",
        "step.completed" => "Step completed",
        "step.blocked" => "Blocked",
        "step.error" => "Error",
        "step.deviation" => "Needs attention",
        "verification.failed" => "Verification failed",
        "tool.error" => "Tool failed",
        "tool.result" => "Tool output",
        "file.changed" => "Code edit",
        "workspace.restored" => "Restored",
        "permission.decision" => "Approval",
        "artifact.created" => "Artifact",
        "branch.forked" => "Restart",
        other => other,
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(5),
        Constraint::Length(2),
    ]).split(area);

    let (workflow, status, step) = match app.session() {
        Some(session) => (
            session.workflow.as_str(),
            session.status.as_str(),
            session.current_step.as_str(),
        ),
        None => ("No session", "idle", "-"),
    };
    frame.render_widget(
        Paragraph::new(format!(
            " Greenlight  │  {workflow}  │  {}  │  step: {step}",
            status.to_uppercase()
        ))
        .block(border(" Workflow ", false)),
        outer[0],
    );

    let columns = Layout::horizontal([
        Constraint::Percentage(27),
        Constraint::Percentage(73),
    ]).split(outer[1]);

    let sessions = app.sessions.iter().map(|session| {
        ListItem::new(format!(
            "{}  [{}]\n{}\n{}",
            session.workflow,
            session.status,
            session.current_step,
            session.id,
        ))
    });
    let sessions_list = List::new(sessions)
        .block(border(" Sessions ", app.focus == Focus::Sessions))
        .highlight_symbol("▶ ")
        .highlight_style(
            Style::default().fg(Color::Black).bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        );
    let mut session_state = ListState::default();
    if !app.sessions.is_empty() {
        session_state.select(Some(app.session_index));
    }
    frame.render_stateful_widget(sessions_list, columns[0], &mut session_state);

    if let Some(prompt) = &app.approval {
        frame.render_widget(
            Paragraph::new(prompt.clone())
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(border(" Approval required ", true)),
            columns[1],
        );
    } else if app.restart_plan.is_some() {
        frame.render_widget(
            Paragraph::new(app.restart_preview.clone())
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(border(" Restart preview — y confirm / n cancel ", true)),
            columns[1],
        );
    } else if !app.restart_choices.is_empty() {
        let choices = app.restart_choices.iter().map(|candidate| {
            ListItem::new(format!(
                "{}  — restore before event #{}",
                candidate.step, candidate.prior_event
            ))
        });
        let choices_widget = List::new(choices)
            .block(border(" Restart from step — Enter to preview / Esc cancel ", true))
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
        let mut state = ListState::default();
        state.select(Some(app.restart_cursor));
        frame.render_stateful_widget(choices_widget, columns[1], &mut state);
    } else {
        let right = Layout::vertical([
            Constraint::Percentage(54),
            Constraint::Percentage(46),
        ]).split(columns[1]);
        let events = app.events.iter().map(|event| {
            let headline = event.data.lines().next().unwrap_or("");
            ListItem::new(Line::from(format!(
                "#{:<5} {:<12} {:<20} {}",
                event.id,
                event.step,
                conversational_label(&event.kind),
                headline
            )))
        });
        let title = if app.show_all { " Conversation · full " } else { " Conversation " };
        let event_list = List::new(events)
            .block(border(title, app.focus == Focus::Events))
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default().fg(Color::Black).bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        let mut event_state = ListState::default();
        if !app.events.is_empty() {
            event_state.select(Some(app.event_index));
        }
        frame.render_stateful_widget(event_list, right[0], &mut event_state);

        let title = if app.diff.is_some() { " Recorded diff " } else { " Message detail " };
        frame.render_widget(
            Paragraph::new(app.detail())
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(border(title, false)),
            right[1],
        );
    }

    let input = if app.composer.is_empty() {
        "Type instructions to resume this session…".to_owned()
    } else {
        app.composer.clone()
    };
    let input = if app.focus == Focus::Compose {
        format!("{input}▌")
    } else {
        input
    };
    let input_title = if app.runner.is_some() {
        " Agent running — input disabled until done "
    } else {
        " Message agent · Enter sends / Ctrl+J newline "
    };
    let composer = Paragraph::new(input)
        .wrap(Wrap { trim: false })
        .block(border(input_title, app.focus == Focus::Compose));
    frame.render_widget(composer, outer[2]);

    let footer = if let Some(prompt) = &app.approval {
        let options = if prompt.contains("[p]") {
            "y once / a process / p project / g global / n deny"
        } else if prompt.contains("[a]") {
            "y once / a process / n deny"
        } else {
            "y approve / n deny"
        };
        format!("Approval: {options}  ·  selected: {}  ·  Enter to confirm", app.approval_choice)
    } else if app.restart_plan.is_some() {
        "Restoration uses the recorded edit journal  ·  y confirm / n cancel  ·  PgUp/PgDn scroll".into()
    } else if !app.restart_choices.is_empty() {
        "j/k choose step  ·  Enter preview  ·  Esc cancel".into()
    } else {
        format!(
            "{}   │   Tab panes  Enter compose  /restart  d diff  v detail  q quit",
            app.notice
        )
    };
    frame.render_widget(Paragraph::new(footer), outer[3]);
}

pub fn run(root: &Path, initial_session: Option<&str>) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("the TUI requires an interactive terminal (TTY)");
    }
    let root: PathBuf = root.canonicalize()?;
    let store = Store::open(&root)?;
    if let Some(id) = initial_session {
        store.session(id).with_context(|| format!("unknown session {id}"))?;
    }
    let mut app = App::new(initial_session);
    app.refresh(&store)?;
    if app.session().is_some_and(|session| session.status == "blocked") {
        app.focus = Focus::Compose;
    }
    ratatui::run(|terminal| -> Result<()> {
        loop {
            app.tick();
            app.refresh(&store)?;
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(150))? {
                if let Event::Key(key) = event::read()? {
                    match app.on_key(key, &root, &store) {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(error) => app.notice = format!("Action error: {error:#}"),
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_opens_composer_and_text_is_editable() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("example", "goal", "plan").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Compose);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), root.path(), &store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), root.path(), &store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.composer, "x");
    }

    #[test]
    fn restart_confirmation_restores_and_creates_fork() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store.create("example", "goal", "plan").unwrap();
        let checkpoint = store.event(&session, Some("plan"), "step.completed", "ok").unwrap();
        store.event(&session, Some("implement"), "step.started", "model").unwrap();
        store.add_change(&session, "implement", "file.txt", Some("before\n"), "after\n").unwrap();
        std::fs::write(root.path().join("file.txt"), "after\n").unwrap();
        session.status = "blocked".into();
        store.save(&session).unwrap();
        let previous_branch = session.active_branch.clone();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.restart_choices = vec![RestartCandidate {
            step: "implement".into(), prior_event: checkpoint,
        }];
        app.prepare_restart(root.path(), &store);
        assert!(app.restart_plan.is_some(), "{}", app.notice);
        app.confirm_restart(root.path(), &store);
        assert_eq!(std::fs::read_to_string(root.path().join("file.txt")).unwrap(), "before\n");
        let fork = store.session(&session.id).unwrap();
        assert_ne!(fork.active_branch, previous_branch);
        assert_eq!(fork.current_step, "implement");
        assert_eq!(fork.status, "ready");
        assert!(app.restart_choices.is_empty());
        assert_eq!(app.focus, Focus::Compose);
    }

    #[test]
    fn navigation_clamps_at_boundaries() {
        assert_eq!(move_index(0, -1, 0), 0);
        assert_eq!(move_index(0, -1, 5), 0);
        assert_eq!(move_index(3, 1, 4), 3);
        assert_eq!(move_index(2, -1, 4), 1);
    }

    #[test]
    fn concise_history_filters_protocol_chatter() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let session = store.create("fix-bug", "test", "explore").unwrap();
        store.event(&session, Some("explore"), "model.output", "raw response").unwrap();
        store.event(&session, Some("explore"), "protocol.error", "bad action").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        assert!(app.events.iter().any(|e| e.kind == "protocol.error"));
        assert!(app.events.iter().all(|e| e.kind != "model.output"));
        app.toggle_verbose();
        app.refresh(&store).unwrap();
        assert!(app.events.iter().any(|e| e.kind == "model.output"));
    }

    #[test]
    fn session_refresh_preserves_selected_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let session = store.create("fix-bug", "test", "explore").unwrap();
        store.event(&session, Some("explore"), "step.started", "start").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.focus = Focus::Events;
        app.navigate(-1);
        let selected = app.event().unwrap().id;
        store.event(&session, Some("explore"), "step.completed", "done").unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.event().unwrap().id, selected);
    }

    #[test]
    fn renders_dashboard_with_test_backend() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        store.create("add-feature", "test", "explore").unwrap();
        let mut app = App::new(None);
        app.refresh(&store).unwrap();
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }

    #[test]
    fn diff_reads_original_journal_snapshots() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let session = store.create("add-feature", "test", "implement").unwrap();
        store.add_change(&session, "implement", "file.txt", Some("old\n"), "new\n").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.toggle_diff(&store).unwrap();
        let detail = app.detail();
        assert!(detail.contains("-old"));
        assert!(detail.contains("+new"));
        app.toggle_diff(&store).unwrap();
        assert!(!app.detail().contains("--- a/file.txt"));
    }
}
