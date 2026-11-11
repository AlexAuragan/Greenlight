//! Read-only live session viewer. The engine remains the sole writer of
//! session history and the sole owner of human approval prompts.
use crate::{
    changes,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::{
    io::{self, IsTerminal},
    path::Path,
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

    fn on_key(&mut self, key: KeyEvent, store: &Store) -> Result<bool> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(false);
        }
        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Sessions => Focus::Events,
                    Focus::Events => Focus::Sessions,
                };
            }
            KeyCode::Esc | KeyCode::Left => self.focus = Focus::Sessions,
            KeyCode::Right | KeyCode::Enter => self.focus = Focus::Events,
            KeyCode::Char('j') | KeyCode::Down => self.navigate(1),
            KeyCode::Char('k') | KeyCode::Up => self.navigate(-1),
            KeyCode::Char('g') | KeyCode::Home => self.select_end(false),
            KeyCode::Char('G') | KeyCode::End => self.select_end(true),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            KeyCode::Char('v') => self.toggle_verbose(),
            KeyCode::Char('d') => self.toggle_diff(store)?,
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

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(area);

    let (workflow, status, step) = match app.session() {
        Some(session) => (
            session.workflow.as_str(),
            session.status.as_str(),
            session.current_step.as_str(),
        ),
        None => ("No session", "idle", "-"),
    };
    let header = Paragraph::new(format!(
        " Greenlight  │  {workflow}  │  {status}  │  step: {step}"
    ))
    .block(border(" Session status ", false));
    frame.render_widget(header, outer[0]);

    let columns = Layout::horizontal([
        Constraint::Percentage(32),
        Constraint::Percentage(68),
    ])
    .split(outer[1]);
    let right = Layout::vertical([
        Constraint::Percentage(46),
        Constraint::Percentage(54),
    ])
    .split(columns[1]);

    let sessions = app.sessions.iter().map(|s| {
        ListItem::new(format!(
            "{}  [{}]\n{}",
            s.workflow,
            s.status,
            s.id,
        ))
    });
    let sessions_list = List::new(sessions)
        .block(border(" Sessions ", app.focus == Focus::Sessions))
        .highlight_symbol("▶ ")
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut session_state = ListState::default();
    if !app.sessions.is_empty() {
        session_state.select(Some(app.session_index));
    }
    frame.render_stateful_widget(sessions_list, columns[0], &mut session_state);

    let events = app.events.iter().map(|e| {
        let first_line = e.data.lines().next().unwrap_or("");
        ListItem::new(Line::from(format!(
            "#{:<5} {:<14} {:<20} {}",
            e.id, e.step, e.kind, first_line
        )))
    });
    let event_title = if app.show_all { " Events · all " } else { " Events · concise " };
    let events_list = List::new(events)
        .block(border(event_title, app.focus == Focus::Events))
        .highlight_symbol("▶ ")
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut event_state = ListState::default();
    if !app.events.is_empty() {
        event_state.select(Some(app.event_index));
    }
    frame.render_stateful_widget(events_list, right[0], &mut event_state);

    let detail_title = if app.diff.is_some() { " Unified diff " } else { " Event detail " };
    let detail = Paragraph::new(app.detail())
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0))
        .block(border(detail_title, false));
    frame.render_widget(detail, right[1]);

    frame.render_widget(
        Paragraph::new(" q quit  │  Tab focus  │  j/k move  │  g/G ends  │  d diff  │  v verbosity  │  PgUp/PgDn detail  │  r refresh"),
        outer[2],
    );
}

pub fn run(root: &Path, initial_session: Option<&str>) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("the TUI requires an interactive terminal (TTY)");
    }
    let store = Store::open(root)?;
    if let Some(id) = initial_session {
        store.session(id).with_context(|| format!("unknown session {id}"))?;
    }
    let mut app = App::new(initial_session);
    app.refresh(&store)?;
    ratatui::run(|terminal| -> Result<()> {
        loop {
            app.refresh(&store)?;
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(600))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('r') {
                        app.refresh(&store)?;
                    } else if app.on_key(key, &store)? {
                        return Ok(());
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
