//! Shared actions, hit regions and terminal input ownership.
use super::*;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    StopManager,
    SendStep,
    SendQuestion,
    SendCommand,
    Commands,
    ManagerReplies,
    Recipient,
    Cancel,
    Continue,
    StartWorkflow,
    ClearGoal,
    BackWorkflow,
    RestoreInstruction,
    Workflow(usize),
    Report,
    ResultEvidence,
    ReviewEdits,
    Versions,
    Provenance,
    Compare,
    Sessions,
    Compose,
    Details,
    Help,
    More,
    Close,
    Mouse,
    Activity,
    Artifacts,
    Edits,
    New,
    Open,
    Copy,
    Editor,
    Filter,
    Usage,
    Follow,
    Stop,
    Preview,
    Expand,
    Caret(u16, u16),
    Decision(char),
    Event(i64),
    Artifact(i64),
    Session(usize),
    Edit(usize),
}
impl Action {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::StopManager => "Stop manager retries",
            Self::SendStep => "Send & resume",
            Self::SendQuestion => "Send question",
            Self::SendCommand => "Run command",
            Self::Commands => "Commands",
            Self::ManagerReplies => "Manager replies",
            Self::Recipient => "Change recipient",
            Self::Cancel => "Cancel",
            Self::Continue => "Continue",
            Self::StartWorkflow => "Start workflow",
            Self::ClearGoal => "Clear draft",
            Self::BackWorkflow => "Back",
            Self::RestoreInstruction => "Restore instructions",
            Self::Report => "Read final report",
            Self::ResultEvidence => "Full result",
            Self::ReviewEdits => "Review edits",
            Self::Versions => "Versions",
            Self::Provenance => "Version details",
            Self::Compare => "Compare previous",
            Self::Sessions => "[f] Sessions",
            Self::Compose => "Compose",
            Self::Details => "[c] Details",
            Self::Help => "[h] Help",
            Self::More => "More",
            Self::Close => "Close Esc",
            Self::Mouse => "Toggle mouse M",
            Self::Activity => "[1] Activity",
            Self::Artifacts => "[2] Artifacts",
            Self::Edits => "[3] Edits",
            Self::New => "New session",
            Self::Open => "Open selected",
            Self::Copy => "Copy",
            Self::Editor => "Editor",
            Self::Filter => "Filter",
            Self::Usage => "Usage",
            Self::Follow => "Follow latest",
            Self::Stop => "Stop requests",
            Self::Preview => "Toggle preview",
            Self::Expand => "Expand preview",
            _ => "Select",
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Hit {
    area: Rect,
    action: Action,
}
pub(super) struct Shell {
    pub project: String,
    pub sessions: bool,
    pub more: bool,
    pub menu_index: usize,
    pub mouse: bool,
    pub preview: bool,
    pub expanded: bool,
    pub actions_focus: bool,
    pub action_index: usize,
    pub request_epoch: u64,
    pub help_scroll: u16,
    pub help_actions: bool,
    pub help_action_index: usize,
    pub(super) hits: RefCell<Vec<Hit>>,
    stamp: RefCell<String>,
    pressed: Option<(Action, String)>,
}
impl Default for Shell {
    fn default() -> Self {
        Self {
            more: false,
            menu_index: 0,
            project: "project".into(),
            sessions: false,
            mouse: true,
            preview: true,
            expanded: false,
            actions_focus: false,
            action_index: 0,
            request_epoch: 0,
            help_scroll: 0,
            help_actions: false,
            help_action_index: 0,
            hits: Default::default(),
            stamp: Default::default(),
            pressed: None,
        }
    }
}
impl Shell {
    pub(super) fn hit(&self, area: Rect, action: Action) {
        if area.width > 0 && area.height > 0 {
            self.hits.borrow_mut().push(Hit { area, action });
        }
    }
}
impl App {
    fn shell_stamp(&self) -> String {
        format!(
            "{:?}:{:?}:{:?}:{:?}:{}:{}:{}:{}:{:?}:{:?}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.session().map(|s| (&s.id, &s.active_branch)),
            self.view,
            self.focus,
            self.event().map(|e| e.id),
            self.shell.request_epoch,
            self.approval.is_some(),
            self.help,
            self.shell.sessions,
            self.artifact().map(|a| a.event_id),
            self.viewport,
            self.result_open,
            self.diagnostic.is_some(),
            self.context_detail.is_some(),
            self.usage_detail.is_some(),
            self.edits.pending.is_some(),
            self.restart_plan.is_some(),
            self.new_session.is_some(),
            self.searching,
            self.activity_filtering,
            self.artifact_browser.is_some()
        ) + &format!(
            ":{}:{}:{:?}",
            self.shell.more, self.shell.menu_index, self.shell.actions_focus
        ) + &format!(":{:?}", self.compose.mode)
            + &format!(
                ":{:?}",
                self.sessions
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>()
            )
            + &format!(
                ":{:?}",
                self.edits
                    .files
                    .iter()
                    .map(|f| (&f.path, f.writes))
                    .collect::<Vec<_>>()
            )
    }
    pub(super) fn begin_frame(&self) {
        self.shell.hits.borrow_mut().clear();
        *self.shell.stamp.borrow_mut() = self.shell_stamp();
    }
    pub(super) fn blocking_surface(&self) -> bool {
        self.help
            || self.approval.is_some()
            || self.shell.more
            || self.searching
            || self.activity_filtering
            || self.new_session.is_some()
            || self.diagnostic.is_some()
            || self.context_detail.is_some()
            || self.usage_detail.is_some()
            || self.restart_plan.is_some()
            || !self.restart_choices.is_empty()
            || self.edits.pending.is_some()
            || self.artifact_browser.is_some()
    }
    pub(super) fn composer_active(&self) -> bool {
        self.focus == Focus::Compose && !self.blocking_surface()
    }
    pub(super) fn more_actions(&self) -> Vec<Action> {
        if self.artifact_browser.is_some() {
            return vec![
                Action::Versions,
                Action::Provenance,
                Action::Compare,
                Action::Copy,
                Action::Editor,
            ];
        }

        let mut actions = vec![
            Action::Activity,
            Action::Artifacts,
            Action::Edits,
            Action::New,
            Action::Usage,
        ];
        if self.focus != Focus::Compose {
            actions.extend([Action::Copy, Action::Editor]);
        }
        if self.view == View::Conversation {
            actions.extend([Action::Filter, Action::Follow]);
        }
        if self.runner.is_some() || self.manager_job.is_some() {
            actions.push(Action::Stop);
        }
        if self.result_open && self.view == View::Conversation {
            actions.insert(0, Action::ResultEvidence);
        }
        if self.view == View::Artifacts && self.artifact().is_some() {
            actions.extend([Action::Versions, Action::Provenance, Action::Compare]);
        }
        if self
            .compose
            .last_instruction
            .as_ref()
            .is_some_and(|(scope, _)| *scope == self.compose.scope)
        {
            actions.push(Action::RestoreInstruction);
        }
        if self.session().is_some() {
            actions.push(Action::ManagerReplies);
        }
        actions.extend([Action::Sessions, Action::Details, Action::Help]);
        actions.extend([Action::Preview, Action::Expand]);
        actions
    }
    pub(super) fn header_actions(&self) -> Vec<Action> {
        if self.blocking_surface() {
            vec![Action::Help]
        } else {
            vec![Action::Sessions, Action::Details, Action::Help]
        }
    }
    pub(super) fn footer_actions(&self) -> Vec<Action> {
        if self.help {
            return vec![Action::Close, Action::Mouse];
        }
        if self.approval.is_some() {
            return vec![Action::Help];
        }
        if self.new_session.is_some() {
            return if self.new_session.as_ref().is_some_and(|f| f.goal.is_some()) {
                vec![
                    Action::StartWorkflow,
                    Action::BackWorkflow,
                    Action::ClearGoal,
                    Action::Cancel,
                ]
            } else {
                if self
                    .new_session
                    .as_ref()
                    .and_then(|f| f.workflows.get(f.cursor))
                    .is_some_and(|w| w.error.is_none())
                {
                    vec![Action::Continue, Action::Cancel]
                } else {
                    vec![Action::Cancel]
                }
            };
        }
        if self.composer_active() {
            if self.compose.mode == compose::Mode::Manager && self.manager_job.is_some() {
                return vec![Action::StopManager, Action::Recipient, Action::Close];
            }
            if self.compose.mode == compose::Mode::Step && !self.can_resume_message() {
                return vec![Action::Recipient, Action::Commands, Action::Close];
            }
            let send = match self.compose.mode {
                compose::Mode::Step => Action::SendStep,
                compose::Mode::Manager => Action::SendQuestion,
                compose::Mode::Command => Action::SendCommand,
            };
            return vec![
                send,
                if self.compose.mode == compose::Mode::Manager && !self.can_resume_message() {
                    Action::Commands
                } else {
                    Action::Recipient
                },
                Action::Close,
            ];
        }
        if self.artifact_browser.is_some() {
            return vec![Action::Versions, Action::Provenance, Action::Close];
        }
        if self.blocking_surface() {
            return vec![Action::Help, Action::Close];
        }
        if self.session().is_none() {
            return vec![Action::New, Action::More];
        }
        if self.result_open && self.view == View::Conversation {
            return vec![
                if self
                    .result_overview
                    .as_ref()
                    .is_some_and(|r| r.report.is_some())
                {
                    Action::Report
                } else {
                    Action::ResultEvidence
                },
                Action::ReviewEdits,
                Action::More,
            ];
        }
        if self.view == View::Artifacts {
            if self.artifact().is_none() {
                return vec![Action::Activity, Action::More];
            }
            return vec![
                if self.focus == Focus::Detail {
                    Action::Close
                } else {
                    Action::Open
                },
                Action::Versions,
                Action::More,
            ];
        }
        vec![Action::Open, Action::Compose, Action::More]
    }
    pub(super) fn shell_actions(&self) -> Vec<Action> {
        let mut actions = self.footer_actions();
        if !self.blocking_surface() || self.new_session.is_some() {
            actions.extend(self.header_actions());
        }
        actions
    }
    pub(super) fn open_artifact_mode(
        &mut self,
        store: &Store,
        mode: crate::artifact_history::Mode,
    ) -> Result<()> {
        let session = self.session().cloned().context("No selected session")?;
        if self.artifact_browser.is_none() {
            let artifact = self.artifact().context("No selected artifact")?;
            let mut browser =
                crate::artifact_history::Browser::open(store, &session, &artifact.name)?;
            browser.index = browser
                .versions
                .iter()
                .position(|v| v.event_id == artifact.event_id)
                .context("Selected artifact version unavailable")?;
            browser.load(store, &session);
            self.artifact_browser = Some(browser);
        }
        let browser = self.artifact_browser.as_mut().unwrap();
        if browser.session != session.id || browser.branch != session.active_branch {
            bail!("Artifact session/branch changed")
        }
        browser.mode = mode;
        browser.reading = mode != crate::artifact_history::Mode::Content;
        browser.load(store, &session);
        self.detail_scroll = 0;
        Ok(())
    }
    pub(super) fn activate(&mut self, action: Action, root: &Path, store: &Store) -> Result<bool> {
        use Action::*;
        self.shell.actions_focus = false;
        if self.composer_active()
            && !matches!(
                action,
                Help | Mouse
                    | Caret(_, _)
                    | Close
                    | SendStep
                    | SendQuestion
                    | SendCommand
                    | Recipient
                    | RestoreInstruction
                    | Commands
            )
        {
            self.focus = self.focus_before_compose;
        }
        if !matches!(action, Help | Close | Mouse) {
            self.shell.more = false;
        }
        let code = match action {
            SendStep | SendQuestion | SendCommand => {
                let expected = match action {
                    SendStep => compose::Mode::Step,
                    SendQuestion => compose::Mode::Manager,
                    _ => compose::Mode::Command,
                };
                if self.compose.mode != expected || self.focus != Focus::Compose {
                    bail!("Message target changed; review the recipient before sending")
                }
                self.send_message(root, store);
                return Ok(false);
            }
            StopManager => {
                self.cancel_manager();
                return Ok(false);
            }
            Commands => {
                self.set_message_mode(compose::Mode::Command);
                self.focus_composer();
                if self.composer.is_empty() {
                    self.composer.push('/');
                }
                return Ok(false);
            }
            ManagerReplies => {
                self.result_open = false;
                self.view = View::Conversation;
                self.activity_query = "manager.".into();
                self.filter_activity();
                self.commit_activity_filter();
                self.focus = Focus::Events;
                self.follow_activity = false;
                return Ok(false);
            }
            Recipient => {
                self.choose_recipient();
                return Ok(false);
            }
            RestoreInstruction => {
                self.restore_instruction();
                return Ok(false);
            }
            Continue | StartWorkflow => KeyCode::Enter,
            Cancel => KeyCode::Esc,
            ClearGoal => {
                self.compose.creation_goal.clear();
                if let Some(form) = &mut self.new_session {
                    form.goal = Some(String::new());
                    form.error = None;
                }
                self.goal_editor.reset();
                return Ok(false);
            }
            BackWorkflow => {
                self.remember_creation();
                if let Some(form) = &mut self.new_session {
                    form.goal = None;
                    form.error = None;
                }
                return Ok(false);
            }
            Workflow(index) => {
                if let Some(form) = &mut self.new_session {
                    if index < form.workflows.len() {
                        form.cursor = index;
                        form.error = None;
                    }
                }
                return Ok(false);
            }
            ResultEvidence => {
                self.context_detail = Some(self.result_text.clone());
                self.detail_scroll = 0;
                return Ok(false);
            }
            Report => {
                let (name, id) = self
                    .result_overview
                    .as_ref()
                    .and_then(|r| r.report.clone())
                    .context("No final report recorded")?;
                let session = self.session().context("No selected session")?;
                let mut browser = crate::artifact_history::Browser::open(store, session, &name)?;
                browser.index = browser
                    .versions
                    .iter()
                    .position(|v| v.event_id == id)
                    .context("Final report version no longer visible")?;
                browser.reading = true;
                browser.load(store, session);
                self.artifact_browser = Some(browser);
                self.detail_scroll = 0;
                return Ok(false);
            }
            ReviewEdits => KeyCode::Char('3'),
            Versions => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Content)?;
                return Ok(false);
            }
            Provenance => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Provenance)?;
                return Ok(false);
            }
            Compare => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Diff)?;
                return Ok(false);
            }
            Sessions => {
                self.shell.sessions = !self.shell.sessions;
                self.focus = if self.shell.sessions {
                    Focus::Sessions
                } else {
                    self.main_focus()
                };
                return Ok(false);
            }
            Compose => {
                self.focus_composer();
                return Ok(false);
            }
            Details => KeyCode::Char('c'),
            Help => {
                self.open_help();
                return Ok(false);
            }
            More => {
                self.shell.more = true;
                self.shell.menu_index = 0;
                return Ok(false);
            }
            Close => KeyCode::Esc,
            Mouse => {
                self.shell.mouse = !self.shell.mouse;
                return Ok(false);
            }
            Activity => KeyCode::Char('1'),
            Artifacts => KeyCode::Char('2'),
            Edits => KeyCode::Char('3'),
            New => KeyCode::Char('n'),
            Open => KeyCode::Enter,
            Copy => KeyCode::Char('C'),
            Editor => KeyCode::Char('E'),
            Filter => KeyCode::Char('F'),
            Usage => {
                let draft = std::mem::replace(&mut self.composer, "/usage".into());
                self.submit(root, store);
                self.composer = draft;
                return Ok(false);
            }
            Follow => KeyCode::End,
            Stop => KeyCode::Char('x'),
            Preview => {
                self.shell.preview = !self.shell.preview;
                return Ok(false);
            }
            Expand => {
                self.shell.expanded = !self.shell.expanded;
                return Ok(false);
            }
            Caret(x, y) => {
                if let Some(goal) = self.new_session.as_ref().and_then(|f| f.goal.as_ref()) {
                    self.goal_editor.click(goal, x, y);
                } else if self.searching {
                    self.session_editor.click(&self.session_query, x, y);
                } else if self.activity_filtering {
                    self.filter_editor.click(&self.activity_query, x, y);
                } else if self.composer_active() {
                    self.composer_editor.click(&self.composer, x, y);
                }
                return Ok(false);
            }
            Decision(c) => KeyCode::Char(c),
            Event(id) => {
                if let Some(i) = self.events.iter().position(|e| e.id == id) {
                    self.event_index = i;
                    self.focus = Focus::Detail;
                    self.inspector_open = true;
                    self.follow_activity = false;
                    self.detail_scroll = 0;
                }
                return Ok(false);
            }
            Artifact(id) => {
                if let Some(i) = self.artifacts.iter().position(|a| a.event_id == id) {
                    self.artifact_index = i;
                    self.focus = Focus::Detail;
                    self.inspector_open = true;
                    self.detail_scroll = 0;
                }
                return Ok(false);
            }
            Edit(i) => {
                if i < self.edits.files.len() {
                    self.edits.index = i;
                    self.edits.select(0);
                    self.focus = Focus::Detail;
                    self.inspector_open = true;
                    self.detail_scroll = 0;
                }
                return Ok(false);
            }
            Session(i) => {
                if i < self.sessions.len() {
                    self.session_index = i;
                    self.shell.sessions = false;
                    self.focus = self.main_focus();
                }
                return Ok(false);
            }
        };
        self.on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
    }
    pub(super) fn on_event(&mut self, event: Event, root: &Path, store: &Store) -> Result<bool> {
        match event {
            Event::Key(key) => {
                self.shell.pressed = None;
                self.on_key(key, root, store)
            }
            Event::Resize(w, h) => {
                self.viewport = (w, h);
                self.shell.hits.borrow_mut().clear();
                self.shell.pressed = None;
                Ok(false)
            }
            Event::Paste(text) => {
                self.shell.pressed = None;
                if self.help
                    || self.approval.is_some()
                    || self.edits.pending.is_some()
                    || self.restart_plan.is_some()
                {
                    return Ok(false);
                }
                if let Some(goal) = self.new_session.as_mut().and_then(|f| f.goal.as_mut()) {
                    self.goal_editor.paste(goal, &text, true, 8192)
                } else if self.searching {
                    self.session_editor
                        .paste(&mut self.session_query, &text, false, 256);
                    self.refresh(store)?;
                } else if self.activity_filtering {
                    self.filter_editor
                        .paste(&mut self.activity_query, &text, false, 256);
                    self.filter_activity();
                } else if self.composer_active() {
                    let limit = self.message_limit();
                    self.composer_editor
                        .paste(&mut self.composer, &text, true, limit)
                }
                Ok(false)
            }
            Event::Mouse(mouse) if self.shell.mouse => {
                if *self.shell.stamp.borrow() != self.shell_stamp() {
                    self.shell.pressed = None;
                    return Ok(false);
                }
                let mut hit = self
                    .shell
                    .hits
                    .borrow()
                    .iter()
                    .rev()
                    .find(|h| h.area.contains((mouse.column, mouse.row).into()))
                    .map(|h| h.action);
                if hit.is_none() && !self.help && self.approval.is_none() {
                    let editor = if self.new_session.as_ref().is_some_and(|f| f.goal.is_some()) {
                        Some(&self.goal_editor)
                    } else if self.searching {
                        Some(&self.session_editor)
                    } else if self.activity_filtering {
                        Some(&self.filter_editor)
                    } else if self.composer_active() {
                        Some(&self.composer_editor)
                    } else {
                        None
                    };
                    if editor.is_some_and(|e| e.contains(mouse.column, mouse.row)) {
                        hit = Some(Action::Caret(mouse.column, mouse.row));
                    }
                }
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.shell.pressed = hit.map(|a| (a, self.shell_stamp()))
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        if let Some((action, stamp)) = self.shell.pressed.take() {
                            if hit == Some(action) && stamp == self.shell_stamp() {
                                return self.activate(action, root, store);
                            }
                        }
                    }
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                        self.shell.pressed = None;
                        if !self.reading_overlay() && !self.composer_active() {
                            match hit {
                                Some(Action::Event(_)) => self.focus = Focus::Events,
                                Some(Action::Artifact(_) | Action::Edit(_)) => {
                                    self.focus = Focus::Artifacts
                                }
                                Some(Action::Session(_)) => self.focus = Focus::Sessions,
                                _ => {}
                            }
                        }
                        return self.on_key(
                            KeyEvent::new(
                                if mouse.kind == MouseEventKind::ScrollDown {
                                    KeyCode::Down
                                } else {
                                    KeyCode::Up
                                },
                                KeyModifiers::NONE,
                            ),
                            root,
                            store,
                        );
                    }
                    _ => self.shell.pressed = None,
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}
/// Selection in the main view tabs is separate from action-button focus.
pub(super) fn tab(frame: &mut Frame, app: &App, area: Rect, action: Action, selected: bool) {
    let style = if selected {
        Style::default()
            .fg(theme::FOCUS)
            .bg(theme::BACKGROUND)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT).bg(theme::PANEL)
    };
    frame.render_widget(
        Paragraph::new(action.label())
            .alignment(ratatui::layout::Alignment::Center)
            .style(style),
        area,
    );
    app.shell.hit(area, action);
}
pub(super) fn button(frame: &mut Frame, app: &App, area: Rect, action: Action, focused: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let primary = matches!(
        action,
        Action::Report
            | Action::New
            | Action::Open
            | Action::SendStep
            | Action::SendQuestion
            | Action::SendCommand
            | Action::Continue
            | Action::StartWorkflow
    );
    // Primary action emphasis is independent from keyboard focus.
    let keyboard_focus = focused;
    let base = if keyboard_focus {
        Style::default()
            .fg(theme::BACKGROUND)
            .bg(theme::FOCUS)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(if primary { theme::FOCUS } else { theme::TEXT })
            .bg(theme::PANEL)
    };
    let available = area.width.saturating_sub(if area.height >= 3 {
        4
    } else if keyboard_focus {
        2
    } else {
        0
    }) as usize;
    let label = if action.label().len() > available {
        match action {
            Action::SendStep | Action::SendQuestion => "Send",
            Action::SendCommand => "Run",
            Action::Recipient => "To",
            Action::StartWorkflow => "Start",
            Action::ClearGoal => "Clear",
            Action::Continue => "Next",
            Action::Help => "Help",
            Action::Report => "Report",
            Action::ReviewEdits => "Edits",
            Action::Open => "Open",
            Action::Close => "Back",
            Action::Mouse => "Mouse",
            _ => action.label(),
        }
    } else {
        action.label()
    };
    let label = super::view::fit(label, available);
    let label = if keyboard_focus {
        format!("› {label}")
    } else {
        label
    };
    let mut paragraph = Paragraph::new(label)
        .alignment(ratatui::layout::Alignment::Center)
        .style(base);
    if area.height >= 3 {
        paragraph = paragraph.block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(if focused || primary {
                    theme::FOCUS
                } else {
                    theme::BORDER
                })),
        );
    }
    frame.render_widget(paragraph, area);
    app.shell.hit(area, action);
}
pub(super) fn set_mouse(enabled: bool) -> Result<()> {
    if enabled {
        crossterm::execute!(io::stdout(), event::EnableMouseCapture)?
    } else {
        crossterm::execute!(io::stdout(), event::DisableMouseCapture)?
    }
    Ok(())
}
pub(super) struct CaptureGuard;
impl CaptureGuard {
    pub(super) fn start() -> Result<Self> {
        let guard = Self;
        crossterm::execute!(
            io::stdout(),
            event::EnableBracketedPaste,
            event::EnableMouseCapture
        )?;
        Ok(guard)
    }
}
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            io::stdout(),
            event::DisableMouseCapture,
            event::DisableBracketedPaste,
            crossterm::cursor::SetCursorStyle::DefaultUserShape
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::MouseEvent;
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::mpsc;
    fn render(app: &mut App, w: u16, h: u16) -> String {
        app.viewport = (w, h);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| view::draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .chunks(w.max(1) as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn key(app: &mut App, root: &Path, store: &Store, code: KeyCode) {
        app.on_event(
            Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            root,
            store,
        )
        .unwrap();
    }
    fn mouse(app: &mut App, root: &Path, store: &Store, kind: MouseEventKind, x: u16, y: u16) {
        app.on_event(
            Event::Mouse(MouseEvent {
                kind,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
            root,
            store,
        )
        .unwrap();
    }
    #[test]
    fn compact_workspace_keeps_content_and_drafts_without_permanent_auxiliary_panes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "A user goal", "work").unwrap();
        store
            .event(&session, None, "user.message", "Recorded evidence")
            .unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.composer = "Unsent draft".into();
        for (w, h) in [(80, 24), (120, 36), (40, 14)] {
            let text = render(&mut app, w, h);
            assert!(!text.contains("Unsent draft"));
            assert!(!text.contains("Last:"));
            assert!(!text.contains("Step context"));
            assert!(text.contains("Compose"));
        }
        key(&mut app, root.path(), &store, KeyCode::Char('i'));
        assert!(render(&mut app, 80, 24).contains("Unsent draft"));
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert_eq!(app.composer, "Unsent draft");
    }
    #[test]
    fn help_is_above_approval_and_expiry_never_restores_an_actionable_request() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        let (_pt, prompts) = mpsc::channel();
        let (answers, received) = mpsc::channel();
        let (ft, finished) = mpsc::channel();
        app.runner = Some(Runner {
            live: None,
            cancel_retry: None,
            session_id: None,
            prompts,
            answers,
            finished,
        });
        app.approval = Some(ApprovalPrompt {
            command: Some("ls".into()),
            content: "Scope: exact command".into(),
            mode: ApprovalMode::Persistent,
        });
        app.detail_scroll = 3;
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        let text = render(&mut app, 80, 24);
        assert!(text.contains("Approval pending"));
        for c in ['y', 'a', 'p', 'g', 'n'] {
            key(&mut app, root.path(), &store, KeyCode::Char(c));
        }
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(app.approval.is_some());
        assert_eq!(app.detail_scroll, 3);
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        ft.send(Err("fixture disconnected".into())).unwrap();
        app.tick();
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(app.approval.is_none());
        assert!(app.runner.is_none());
    }
    #[test]
    fn paste_and_caret_click_are_literal_and_keep_graphemes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        app.focus_composer();
        app.composer = "é文".into();
        render(&mut app, 80, 24);
        app.composer_editor.key(
            &mut app.composer,
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            true,
            8192,
        );
        app.on_event(
            Event::Paste("h\nj/k\n/resume\x1b".into()),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.composer, "h\nj/k\n/resumeé文");
        assert!(!app.help);
        assert!(app.runner.is_none());
        render(&mut app, 80, 24);
        let (area, offset) = app.composer_editor.drawn.get().unwrap();
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Down(MouseButton::Left),
            area.x,
            area.y,
        );
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Up(MouseButton::Left),
            area.x,
            area.y,
        );
        key(&mut app, root.path(), &store, KeyCode::Char('!'));
        assert!(app.composer.lines().nth(offset).unwrap().starts_with('!'));
    }
    #[test]
    fn mouse_release_rejects_replaced_request_and_resize_targets() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        let (_pt, prompts) = mpsc::channel();
        let (answers, received) = mpsc::channel();
        let (_ft, finished) = mpsc::channel();
        app.runner = Some(Runner {
            live: None,
            cancel_retry: None,
            session_id: None,
            prompts,
            answers,
            finished,
        });
        app.approval = Some(ApprovalPrompt {
            command: None,
            content: "First".into(),
            mode: ApprovalMode::Human,
        });
        render(&mut app, 80, 24);
        let hit = app
            .shell
            .hits
            .borrow()
            .iter()
            .find(|h| h.action == Action::Decision('y'))
            .unwrap()
            .clone();
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Down(MouseButton::Left),
            hit.area.x,
            hit.area.y,
        );
        app.shell.request_epoch += 1;
        app.approval.as_mut().unwrap().content = "Replacement".into();
        render(&mut app, 80, 24);
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Up(MouseButton::Left),
            hit.area.x,
            hit.area.y,
        );
        assert!(received.try_recv().is_err());
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Down(MouseButton::Left),
            hit.area.x,
            hit.area.y,
        );
        app.on_event(Event::Resize(100, 30), root.path(), &store)
            .unwrap();
        render(&mut app, 100, 30);
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Up(MouseButton::Left),
            hit.area.x,
            hit.area.y,
        );
        assert!(received.try_recv().is_err());
        render(&mut app, 100, 30);
        let deny = app
            .shell
            .hits
            .borrow()
            .iter()
            .find(|h| h.action == Action::Decision('n'))
            .unwrap()
            .clone();
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Down(MouseButton::Left),
            deny.area.x,
            deny.area.y,
        );
        mouse(
            &mut app,
            root.path(),
            &store,
            MouseEventKind::Up(MouseButton::Left),
            deny.area.x,
            deny.area.y,
        );
        assert_eq!(received.try_recv().unwrap(), "n");
        assert!(app.approval.is_none());
    }
    #[test]
    fn help_controls_are_focusable_and_tiny_layers_do_not_panic() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        for (w, h) in [(0, 0), (1, 1), (3, 3), (40, 14), (80, 24)] {
            render(&mut app, w, h);
        }
        key(&mut app, root.path(), &store, KeyCode::BackTab);
        key(&mut app, root.path(), &store, KeyCode::Enter);
        assert!(!app.shell.mouse);
        assert!(app.help);
        key(&mut app, root.path(), &store, KeyCode::Tab);
        key(&mut app, root.path(), &store, KeyCode::Enter);
        assert!(!app.help);
    }
    #[test]
    fn toolbar_and_more_are_reachable_without_function_keys_and_do_not_submit_on_focus() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        key(&mut app, root.path(), &store, KeyCode::BackTab);
        assert!(app.shell.actions_focus);
        assert!(app.runner.is_none());
        app.shell.action_index = app
            .shell_actions()
            .iter()
            .position(|a| *a == Action::More)
            .unwrap();
        key(&mut app, root.path(), &store, KeyCode::Enter);
        assert!(app.shell.more);
        key(&mut app, root.path(), &store, KeyCode::Tab);
        key(&mut app, root.path(), &store, KeyCode::Enter);
        assert_eq!(app.view, View::Artifacts);
        assert!(!app.shell.more);
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        key(&mut app, root.path(), &store, KeyCode::Char('M'));
        assert!(!app.shell.mouse);
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(!app.help);
    }
}
