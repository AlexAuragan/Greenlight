//! Interactive session viewer: conversation, artifacts and human decisions.
//! The engine and store remain the authorities for tool execution and history.
use crate::{
    changes,
    restore::RestorePlan,
    store::{Artifact, Session, Store},
    tools::{ApprovalMode, ApprovalPrompt},
    tui_actions::{self, RestartCandidate, Runner},
};
use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
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
    Artifacts,
    Detail,
    Compose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Conversation,
    Artifacts,
}

struct App {
    sessions: Vec<Session>,
    session_index: usize,
    events: Vec<EventRow>,
    event_index: usize,
    artifacts: Vec<Artifact>,
    artifact_index: usize,
    view: View,
    render_markdown: bool,
    focus: Focus,
    focus_before_compose: Focus,
    show_all: bool,
    detail_scroll: u16,
    diff: Option<(i64, String)>,
    requested_session: Option<String>,
    composer: String,
    notice: String,
    runner: Option<Runner>,
    approval: Option<ApprovalPrompt>,
    restart_choices: Vec<RestartCandidate>,
    restart_cursor: usize,
    restart_plan: Option<RestorePlan>,
    restart_preview: String,
    restart_branch: Option<String>,
    diagnostic: Option<String>,
    diagnostic_key: Option<(String,String)>,
    artifact_diagnostic_key: Option<(String,String)>,
    usage_line: String,
    usage_detail: Option<String>,
    usage_error: Option<String>,
    workspace: Option<crate::workspace_view::SessionWorkspace>,
    workspace_error: Option<String>,
    workspace_session: Option<String>,
    context_detail: Option<String>,
    blocker_card: bool,
}

impl App {
    fn new(initial_session: Option<&str>) -> Self {
        Self {
            sessions: Vec::new(),
            session_index: 0,
            events: Vec::new(),
            event_index: 0,
            artifacts: Vec::new(),
            artifact_index: 0,
            view: View::Conversation,
            render_markdown: true,
            focus: if initial_session.is_some() {
                Focus::Events
            } else {
                Focus::Sessions
            },
            focus_before_compose: if initial_session.is_some() {
                Focus::Events
            } else {
                Focus::Sessions
            },
            show_all: false,
            detail_scroll: 0,
            diff: None,
            requested_session: initial_session.map(str::to_owned),
            composer: String::new(),
            notice: "←/→ panes · ↑/↓ navigate/scroll · i compose · Enter open".into(),
            runner: None,
            approval: None,
            restart_choices: Vec::new(),
            restart_cursor: 0,
            restart_plan: None,
            restart_preview: String::new(),
            restart_branch: None,
            diagnostic: None,
            diagnostic_key: None,
            artifact_diagnostic_key: None,
            usage_line: "Usage: no recorded model requests".into(),
            usage_detail: None,
            usage_error: None,
            workspace: None,
            workspace_error: None,
            workspace_session: None,
            context_detail: None,
            blocker_card: false,
        }
    }
    fn raise_diagnostic(&mut self, message: String) {
        self.notice = format!("Attention required: {}",message.lines().next().unwrap_or("inspect diagnostic"));
        self.diagnostic = Some(message);
        self.detail_scroll = 0;
    }

    fn session(&self) -> Option<&Session> {
        self.sessions.get(self.session_index)
    }

    fn event(&self) -> Option<&EventRow> {
        self.events.get(self.event_index)
    }
    fn inspect_blocker(&mut self) {
        if let Some(id)=self.workspace.as_ref().and_then(|w|w.blocker.as_ref()).map(|b|b.event_id) {
            if let Some(index)=self.events.iter().position(|e|e.id==id) {self.event_index=index;}
        }
        self.blocker_card=false;
        self.view=View::Conversation;
        self.focus=Focus::Detail;
        self.detail_scroll=0;
    }

    fn artifact(&self) -> Option<&Artifact> {
        self.artifacts.get(self.artifact_index)
    }

    fn switch_view(&mut self, view: View) {
        if self.view == view {
            return;
        }
        self.view = view;
        if self.focus != Focus::Compose && self.focus != Focus::Detail {
            self.focus = match view {
                View::Conversation => Focus::Events,
                View::Artifacts => Focus::Artifacts,
            };
        }
        self.detail_scroll = 0;
    }

    fn main_focus(&self) -> Focus {
        match self.view {
            View::Conversation => Focus::Events,
            View::Artifacts => Focus::Artifacts,
        }
    }

    fn focus_composer(&mut self) {
        if self.focus != Focus::Compose {
            self.focus_before_compose = self.focus;
        }
        self.focus = Focus::Compose;
    }

    fn focus_right(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => self.main_focus(),
            Focus::Events | Focus::Artifacts => Focus::Detail,
            other => other,
        };
    }

    fn focus_left(&mut self) {
        self.focus = match self.focus {
            Focus::Detail => self.main_focus(),
            Focus::Events | Focus::Artifacts => Focus::Sessions,
            other => other,
        };
    }

    fn scroll_detail(&mut self, delta: i16) {
        self.detail_scroll = self.detail_scroll.saturating_add_signed(delta);
    }

    fn refresh(&mut self, store: &Store) -> Result<()> {
        let old_session = self.session().map(|s| s.id.clone());
        let old_event = self.event().map(|e| e.id);
        let old_artifact = self.artifact().map(|a| a.name.clone());
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
        if let Some(session) = self.session().cloned() {
            let history = store.history(&session)?;
            let session_changed=self.workspace_session.as_deref()!=Some(session.id.as_str());
            let old_primary=self.workspace.as_ref().map(|w|w.primary);
            let old_blocker=self.workspace.as_ref().and_then(|w|w.blocker.as_ref()).map(|b|b.event_id);
            self.workspace_session=Some(session.id.clone());
            match crate::workspace_view::SessionWorkspace::load(store,&session,&history) {
                Ok(workspace)=>{
                    let new_blocker=workspace.blocker.as_ref().map(|b|b.event_id);
                    if session_changed || new_blocker!=old_blocker || old_primary!=Some(workspace.primary) {
                        self.blocker_card=new_blocker.is_some() || matches!(workspace.primary,crate::workspace_view::PrimaryAction::RecoverOwner|crate::workspace_view::PrimaryAction::RecoverWrites);
                    }
                    self.workspace=Some(workspace);self.workspace_error=None;
                }
                Err(error)=>{self.workspace=None;self.workspace_error=Some(format!("Workspace projection unavailable: {error:#}"));}
            }
            if session_changed {self.context_detail=None;}
            let show_all = self.show_all;
            self.events = history
                .into_iter()
                .map(|(id, step, kind, data)| {
                    if let Some(error) = invalid_event_payload(&kind,&data) {
                        EventRow {id,step,kind:"event.invalid".into(),data:format!("Malformed {kind} event #{id}: {error}\n\nRaw evidence:\n{data}")}
                    } else { EventRow {id,step,kind,data} }
                })
                .filter(|event| show_all || !is_chatter(&event.kind))
                .collect();
            if session_changed { self.diagnostic = None; self.usage_detail=None; }
            match store.usage_report(&session,crate::usage::Scope::Session) {
                Ok(report)=>{self.usage_line=report.totals.compact();self.usage_error=None;if self.usage_detail.is_some(){self.usage_detail=Some(report.display());}}
                Err(error)=>{
                    self.usage_line="Usage unavailable — inspect accounting records".into();
                    let message=format!("Usage accounting could not be read: {error:#}");
                    if self.usage_error.as_ref()!=Some(&message) {self.usage_error=Some(message.clone());self.raise_diagnostic(message);}
                }
            }
            let last_restore_outcome=self.events.iter().rev().find(|e|matches!(e.kind.as_str(),"workspace.restore-failed"|"workspace.restore-completed")).map(|e|e.id);
            if let Some(event) = self.events.iter().rev().find(|e|
                e.kind=="event.invalid" || (e.kind=="workspace.restore-failed" && Some(e.id)==last_restore_outcome) ||
                (session.status=="blocked" && matches!(e.kind.as_str(),"step.blocked"|"runner.error"|"step.error"))) {
                let key = (session.id.clone(),format!("event:{}",event.id));
                if self.diagnostic_key.as_ref()!=Some(&key) {
                    let message = format!("{} — event #{}\n\n{}\n\nInspect evidence before retrying. Esc closes this diagnostic.",conversational_label(&event.kind),event.id,event.data);
                    self.diagnostic_key = Some(key);
                    self.raise_diagnostic(message);
                }
            }
            self.artifacts = match store.artifacts(&session,"session") {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    let message = format!("Artifact loading failed: {error:#}\nHistory remains available. Inspect the state directory before retrying.");
                    let key = (session.id.clone(),message.clone());
                    if self.artifact_diagnostic_key.as_ref()!=Some(&key) { self.artifact_diagnostic_key=Some(key); self.raise_diagnostic(message); }
                    Vec::new()
                }
            };
        } else {
            self.events.clear();
            self.artifacts.clear();
            self.workspace=None;self.workspace_error=None;self.workspace_session=None;self.blocker_card=false;self.context_detail=None;
        }
        if changed_session {
            self.artifact_index = 0;
        } else if let Some(name) = old_artifact {
            self.artifact_index = self.artifacts.iter()
                .position(|artifact| artifact.name == name)
                .unwrap_or(0);
        } else {
            self.artifact_index = 0;
        }
        if changed_session || following_tail {
            self.event_index = self.events.len().saturating_sub(1);
            if (changed_session || self.view == View::Conversation) && self.diagnostic.is_none() && self.approval.is_none() && self.usage_detail.is_none() && self.context_detail.is_none() && !self.blocker_card {
                self.detail_scroll = 0;
            }
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
            Focus::Compose | Focus::Detail => (),
            Focus::Sessions => {
                let next = move_index(self.session_index, delta, self.sessions.len());
                if next != self.session_index {
                    self.session_index = next;
                    self.events.clear();
                    self.event_index = 0;
                    self.artifacts.clear();
                    self.artifact_index = 0;
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
            Focus::Artifacts => {
                let next = move_index(self.artifact_index, delta, self.artifacts.len());
                if next != self.artifact_index {
                    self.artifact_index = next;
                    self.detail_scroll = 0;
                }
            }
        }
    }

    fn select_end(&mut self, last: bool) {
        match self.focus {
            Focus::Compose | Focus::Detail => (),
            Focus::Sessions => {
                let next = if last { self.sessions.len().saturating_sub(1) } else { 0 };
                if next != self.session_index {
                    self.session_index = next;
                    self.events.clear();
                    self.event_index = 0;
                    self.artifacts.clear();
                    self.artifact_index = 0;
                    self.detail_scroll = 0;
                    self.diff = None;
                }
            }
            Focus::Events => {
                self.event_index = if last { self.events.len().saturating_sub(1) } else { 0 };
                self.detail_scroll = 0;
                self.diff = None;
            }
            Focus::Artifacts => {
                self.artifact_index = if last { self.artifacts.len().saturating_sub(1) } else { 0 };
                self.detail_scroll = 0;
            }
        }
    }

    fn toggle_verbose(&mut self) {
        self.blocker_card=false;
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
                    if let Some(id)=runner.session_id.as_ref() {
                        if let Some(index)=self.sessions.iter().position(|s|&s.id==id) {
                            self.session_index=index;
                            if self.workspace_session.as_deref()!=Some(id) {self.workspace=None;self.context_detail=None;}
                        }
                        self.requested_session=Some(id.clone());
                    }
                    self.approval = Some(question);
                    self.detail_scroll = 0;
                }
                Err(TryRecvError::Disconnected | TryRecvError::Empty) => {}
            }
        }
        let completion = runner.finished.try_recv();
        let failed = !matches!(&completion, Ok(Ok(()))|Err(TryRecvError::Empty));
        if !matches!(&completion, Err(TryRecvError::Empty)) {
            self.notice = match completion {
                Ok(result) => match result {
                Ok(()) => "Workflow stopped or completed. See its latest events.".into(),
                Err(error) => format!("Runner error: {error}"),
                },
                Err(_) => "Worker disconnected; pending approval expired. Inspect history and use /recover before resuming.".into(),
            };
            self.runner = None;
            self.approval = None;
            if failed { self.raise_diagnostic(self.notice.clone()); }
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
            self.notice = "Type a message to resume; /context shows attempt/input evidence; /usage shows token/cost accounting; /recover recovers a dead owner; /restart selects an earlier step; /events toggles protocol detail; Ctrl+J adds a newline.".into();
            self.composer.clear();
            return;
        }
        if instruction=="/usage" {
            if let Some(session)=self.session() {
                match store.usage_report(session,crate::usage::Scope::Session) {
                    Ok(report)=>{self.usage_detail=Some(report.display());self.detail_scroll=0;}
                    Err(error)=>self.raise_diagnostic(format!("Usage unavailable: {error:#}")),
                }
            }
            self.composer.clear();
            return;
        }
        if instruction=="/context" {
            self.context_detail=Some(self.workspace.as_ref().map(|w|format!("{}\n\nLatest recorded step states (loops may revisit):\n{}",w.context(),w.step_summary())).unwrap_or_else(||self.workspace_error.clone().unwrap_or_else(||"No session selected".into())));
            self.composer.clear();self.detail_scroll=0;return;
        }
        if instruction == "/recover" {
            if self.runner.is_some() {
                self.notice = "Cannot recover while this TUI's worker is active.".into();
                return;
            }
            if let Some(session) = self.session() {
                self.notice = match store.recover_session(&session.id) {
                    Ok(message) => message,
                    Err(error) => format!("Recovery refused: {error:#}"),
                };
            } else { self.notice = "No session selected.".into(); }
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
        if let Err(error) = store.ensure_no_pending_writes() {
            self.notice = format!("Cannot resume: {error:#}");
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
            self.notice = format!("Session is {}; only ready or blocked sessions can resume. If its worker died, use /recover.", session.status);
            return;
        }
        let input = if instruction == "/resume" { "" } else { &instruction };
        self.runner = Some(tui_actions::resume(
            root.to_path_buf(), session.id, input.to_owned(),
        ));
        self.composer.clear();
        self.notice = "Running agent. Incoming approvals will appear here.".into();
        self.focus = self.main_focus();
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
        let branch = session.active_branch.clone();
        match RestorePlan::prepare(root, store, session, candidate.prior_event) {
            Ok(plan) => {
                self.restart_preview = format!(
                    "Restart '{}' from event #{} (the previous checkpoint).\n\n{}",
                    candidate.step, candidate.prior_event, plan.preview_text(),
                );
                self.restart_branch = Some(branch);
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
            self.raise_diagnostic(format!("Restore failed: {error:#}. Inspect history before retrying."));
            self.restart_choices.clear();
            return;
        }
        match store.fork(&mut state, candidate.prior_event, &candidate.step) {
            Ok(()) => self.notice = format!(
                "Forked from event #{} to '{}'. Type an instruction or /resume.",
                candidate.prior_event, candidate.step
            ),
            Err(error) => self.raise_diagnostic(format!(
                "Restore applied, but fork failed: {error:#}. Inspect workspace and history."
            )),
        }
        self.restart_choices.clear();
        self.restart_branch = None;
        self.composer.clear();
        self.focus_composer();
        self.detail_scroll = 0;
    }

    fn deliver_approval(&mut self, choice: char) -> Result<()> {
        let request = self.approval.as_ref().context("no pending approval")?;
        if !request.mode.accepts(choice) {
            bail!("approval option {choice} is unavailable for this request");
        }
        let confirmation = request.mode==ApprovalMode::ConfirmPersistent;
        let runner = self.runner.as_ref().context("approval worker missing")?;
        runner.answers.send(choice.to_string()).context("approval worker disconnected")?;
        self.approval = None;
        self.notice = match choice {
            'y' if confirmation => "Confirmed persistent grant; awaiting engine result.".into(),
            'y' => "Sent approval to the engine; awaiting its recorded outcome.".into(),
            'a' => "Sent process-wide approval to the engine.".into(),
            'p' => "Requested project-wide grant; awaiting engine result.".into(),
            'g' => "Requested global grant; awaiting engine result.".into(),
            'n' => "Denied this request; the agent may attempt another action.".into(),
            _ => unreachable!("choices validated above"),
        };
        Ok(())
    }

    fn on_key(&mut self, key: KeyEvent, root: &Path, store: &Store) -> Result<bool> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(false);
        }

        if let Some(mode) = self.approval.as_ref().map(|request| request.mode) {
            if key.kind != KeyEventKind::Press { return Ok(false); }
            match key.code {
                KeyCode::Char(choice)
                    if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && mode.accepts(choice.to_ascii_lowercase()) =>
                {
                    self.deliver_approval(choice.to_ascii_lowercase())?;
                }
                KeyCode::Esc => self.deliver_approval('n')?,
                // Enter is intentionally inert. Approval must be an explicit
                // choice, never an accidental confirmation of a default.
                KeyCode::Enter => {}
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                _ => {}
            }
            return Ok(false);
        }

        if self.diagnostic.is_some() || self.usage_detail.is_some() || self.context_detail.is_some() {
            match key.code {
                KeyCode::Esc => { self.diagnostic=None; self.usage_detail=None; self.context_detail=None; self.detail_scroll=0; },
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                KeyCode::Char('q') if self.runner.is_none() => return Ok(true),
                _ => {},
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
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
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
                KeyCode::Esc => self.focus = self.focus_before_compose,
                KeyCode::Enter => self.submit(root, store),
                KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.composer.push('\n');
                }
                KeyCode::Backspace => { self.composer.pop(); }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.composer.len() < 8_192 =>
                {
                    self.composer.push(c);
                }
                _ => {}
            }
            return Ok(false);
        }

        match key.code {
            KeyCode::Char('c')=>{self.context_detail=Some(self.workspace.as_ref().map(|w|format!("{}\n\nLatest recorded step states (loops may revisit):\n{}",w.context(),w.step_summary())).unwrap_or_else(||self.workspace_error.clone().unwrap_or_else(||"No session selected".into())));self.detail_scroll=0;},
            KeyCode::Char('e') if self.workspace.as_ref().is_some_and(|w|w.blocker.is_some())=>self.inspect_blocker(),
            KeyCode::Char('h')=>{self.blocker_card=false;self.view=View::Conversation;self.focus=Focus::Events;},
            KeyCode::Char('b')=>{self.blocker_card=true;self.view=View::Conversation;self.focus=Focus::Events;},
            KeyCode::Char('r')=>self.open_restart(root,store),
            KeyCode::Char('q') if self.runner.is_none() => return Ok(true),
            KeyCode::Char('q') => self.notice = "Agent running; wait for its result before exiting.".into(),
            KeyCode::Tab | KeyCode::Right => {
                if self.focus == Focus::Detail && key.code == KeyCode::Tab {
                    self.focus_composer();
                } else {
                    self.focus_right();
                }
            }
            KeyCode::Esc | KeyCode::Left => self.focus_left(),
            KeyCode::Char('1') => self.switch_view(View::Conversation),
            KeyCode::Char('2') => self.switch_view(View::Artifacts),
            KeyCode::Char('m') if self.view == View::Artifacts => {
                self.render_markdown = !self.render_markdown;
                self.detail_scroll = 0;
            },
            KeyCode::Enter => {
                if self.blocker_card && self.view==View::Conversation && self.focus==Focus::Events {
                    self.inspect_blocker();
                } else if self.focus == Focus::Detail {
                    self.focus_composer();
                } else {
                    self.focus_right();
                }
            }
            KeyCode::Char('i') => self.focus_composer(),
            KeyCode::Char('/') => {
                self.focus_composer();
                self.composer.push('/');
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.focus == Focus::Detail || (self.blocker_card && self.view==View::Conversation && self.focus==Focus::Events) { self.scroll_detail(1); }
                else { self.navigate(1); }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus == Focus::Detail || (self.blocker_card && self.view==View::Conversation && self.focus==Focus::Events) { self.scroll_detail(-1); }
                else { self.navigate(-1); }
            }
            KeyCode::Char('g') | KeyCode::Home => {
                if self.focus == Focus::Detail { self.detail_scroll = 0; }
                else { self.select_end(false); }
            }
            KeyCode::Char('G') | KeyCode::End => {
                if self.focus != Focus::Detail { self.select_end(true); }
            }
            KeyCode::Char('v') if self.view == View::Conversation => self.toggle_verbose(),
            KeyCode::Char('d') if self.view == View::Conversation => self.toggle_diff(store)?,
            _ => {}
        }
        Ok(false)
    }

}

fn is_chatter(kind: &str) -> bool {
    matches!(
        kind,
        "step.definition" | "context.assembled" | "model.output" | "tool.request"
            | "permission.decision"
            | "model.call-started" | "model.usage"
            | "workspace.manifest.before" | "workspace.manifest.after"
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
fn safe_display(text:&str)->String {text.chars().filter(|c|!c.is_control() || *c=='\n' || *c=='\t').collect()}
fn inline_display(text:&str)->String {safe_display(text).replace(['\n','\t']," ")}

fn conversational_label(kind: &str) -> &str {
    match kind {
        "user.instruction" => "You",
        "step.started" => "Step started",
        "step.completed" => "Step completed",
        "step.blocked" => "Blocked",
        "step.error" => "Error",
        "event.invalid" => "Malformed event",
        "workspace.restore-failed" => "Restore failed",
        "workspace.restore-completed" => "Restore completed",
        "runner.error" => "Worker error",
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

fn approval_line(key: char, description: &'static str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  [{key}] "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(description),
    ])
}

fn invalid_event_payload(kind: &str, data: &str) -> Option<String> {
    let result: Result<()> = (|| {
        match kind {
            "workspace.manifest.before" | "workspace.manifest.after" => {
                serde_json::from_str::<crate::workspace::Manifest>(data)?;
            }
            "workspace.delta" => { serde_json::from_str::<Vec<crate::workspace::FileDifference>>(data)?; }
            "context.inputs" | "human.approved" => { crate::input_pins::validate_recorded_event(kind,data)?; }
            "verification.evidence" => {
                let value: serde_json::Value = serde_json::from_str(data)?;
                if !value["attempt"].is_string() || !value["artifact_events"].as_array().is_some_and(|a|a.iter().all(|v|v.is_i64())) || !value["observation_events"].as_array().is_some_and(|a|a.iter().all(|v|v.is_i64())) {
                    bail!("invalid verification evidence fields");
                }
            }
            "workspace.restore-failed" => {
                let value: serde_json::Value = serde_json::from_str(data)?;
                if !value["error"].is_string() || !value["changed_files"].is_u64() || !value["journaled_files"].is_u64() { bail!("invalid restore failure fields"); }
            }
            _ => {}, // Unknown event types remain readable as raw evidence.
        }
        Ok(())
    })();
    result.err().map(|error|format!("{error:#}"))
}

fn approval_options(mode: ApprovalMode) -> Vec<Line<'static>> {
    let mut options = vec![approval_line('y', if mode==ApprovalMode::ConfirmPersistent {"Save grant and allow"} else if mode==ApprovalMode::Human {"Approve checkpoint"} else {"Allow once"}, Color::Green)];
    if matches!(mode,ApprovalMode::Tool|ApprovalMode::Persistent) {
        options.push(approval_line('a', "Allow for this process", Color::Cyan));
    }
    if mode == ApprovalMode::Persistent {
        options.push(approval_line('p', "Review project grant", Color::Yellow));
        options.push(approval_line('g', "Review global grant", Color::Yellow));
    }
    options.push(approval_line('n', "Deny (Esc also denies)", Color::Red));
    options.push(Line::styled(
        "  Each key sends immediately. Enter has no effect.",
        Style::default().fg(Color::DarkGray),
    ));
    options
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::vertical([
        Constraint::Length(7), Constraint::Min(6), Constraint::Length(4), Constraint::Length(2),
    ]).split(area);
    let (workflow, raw_status, step) = match app.session() {
        Some(session)=>(session.workflow.as_str(),session.status.as_str(),session.current_step.as_str()),
        None=>("No session","idle","-"),
    };
    let workspace=app.workspace.as_ref().filter(|_|app.workspace_session.as_deref()==app.session().map(|s|s.id.as_str()));
    let status=if workspace.is_some_and(|w|w.primary==crate::workspace_view::PrimaryAction::RecoverOwner){"INTERRUPTED".into()}else{raw_status.to_uppercase()};
    let active_tab=Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let inactive_tab=Style::default().fg(Color::DarkGray);
    let goal=workspace.map(|w|w.goal.as_str()).or_else(||app.session().map(|s|s.goal.as_str())).unwrap_or("Choose a session");
    let attempt=workspace.map(|w|w.attempt_label()).unwrap_or_else(||"context unavailable".into());
    let progress=workspace.map(|w|w.stepper(step,area.width.saturating_sub(2) as usize)).unwrap_or_else(||"Workflow context unavailable; h history remains available".into());
    let next=if app.approval.is_some(){"Decide in the approval pane; Enter never approves"}else{workspace.map(|w|w.primary.label()).unwrap_or("Inspect configuration/history")};
    frame.render_widget(Paragraph::new(vec![
        Line::from(inline_display(&format!(" {goal}"))).style(Style::default().add_modifier(Modifier::BOLD)),
        Line::from(inline_display(&format!(" {workflow} · {status} · {step} · {attempt}"))),
        Line::from(inline_display(&progress)),
        Line::from(inline_display(&format!(" Next: {next}"))).style(Style::default().fg(Color::Yellow)),
        Line::from(vec![Span::styled(" [1] Activity ",if app.view==View::Conversation{active_tab}else{inactive_tab}),
            Span::styled(" [2] Artifacts ",if app.view==View::Artifacts{active_tab}else{inactive_tab}),Span::raw(" c context · h history")]),
    ]).block(border(" Session workspace ",false)),outer[0]);

    let columns = Layout::horizontal([
        Constraint::Percentage(27),
        Constraint::Percentage(73),
    ]).split(outer[1]);

    let sessions = app.sessions.iter().map(|session| {
        ListItem::new(format!("{}\n{} [{}]\n{}\n{}",inline_display(&session.goal),session.workflow,session.status,session.current_step,session.id.chars().take(8).collect::<String>()))
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
        let choices_height = match prompt.mode {
            ApprovalMode::Human | ApprovalMode::ConfirmPersistent => 5,
            ApprovalMode::Tool => 6,
            ApprovalMode::Persistent => 8,
        };
        let approval_areas = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(choices_height),
        ]).split(columns[1]);
        let title = if prompt.mode == ApprovalMode::ConfirmPersistent {
            " Confirm persistent grant "
        } else if prompt.mode == ApprovalMode::Human {
            " Human approval required "
        } else {
            " Tool permission required "
        };
        frame.render_widget(
            Paragraph::new(
                prompt.content.rsplit_once("\n[y]")
                    .map_or(prompt.content.as_str(), |(description, _)| description)
                    .to_string(),
            )
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(border(title, true)),
            approval_areas[0],
        );
        frame.render_widget(
            Paragraph::new(approval_options(prompt.mode))
                .block(border(" Press a key to decide — no Enter needed ", true)),
            approval_areas[1],
        );
    } else if let Some(message) = &app.diagnostic {
        frame.render_widget(Paragraph::new(crate::markdown::render(message))
            .wrap(Wrap {trim:false}).scroll((app.detail_scroll,0))
            .block(Block::default().borders(Borders::ALL).title(" ATTENTION — Esc closes; arrows scroll ").border_style(Style::default().fg(Color::Red))),columns[1]);
    } else if let Some(message)=&app.usage_detail {
        frame.render_widget(Paragraph::new(crate::markdown::render(message)).wrap(Wrap {trim:false}).scroll((app.detail_scroll,0))
            .block(border(" Usage — session spending · Esc closes ",true)),columns[1]);
    } else if let Some(message)=&app.context_detail {
        frame.render_widget(Paragraph::new(crate::markdown::render(&safe_display(message))).wrap(Wrap {trim:false}).scroll((app.detail_scroll,0))
            .block(border(" Attempt context · Esc closes ",true)),columns[1]);
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
    } else if app.view == View::Artifacts {
        let panes = Layout::horizontal([
            Constraint::Percentage(38),
            Constraint::Percentage(62),
        ]).split(columns[1]);
        let items = app.artifacts.iter().map(|artifact| {
            ListItem::new(format!(
                "{}\n  step: {}  ·  {} bytes",
                artifact.name, artifact.step, artifact.content.len()
            ))
        });
        let artifacts = List::new(items)
            .block(
                border(" Artifacts · latest versions ", app.focus == Focus::Artifacts)
                    .title(format!(" {} saved ", app.artifacts.len())),
            )
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default().fg(Color::Black).bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        let mut state = ListState::default();
        if !app.artifacts.is_empty() {
            state.select(Some(app.artifact_index));
        }
        frame.render_stateful_widget(artifacts, panes[0], &mut state);

        let content = match app.artifact() {
            Some(artifact) => {
                let mut lines = vec![
                    Line::styled(
                        artifact.name.clone(),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(
                        format!("Produced by: {}  ·  {} bytes", artifact.step, artifact.content.len()),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Line::default(),
                ];
                if app.render_markdown {
                    lines.extend(crate::markdown::render(&artifact.content).lines);
                } else {
                    lines.extend(Text::raw(artifact.content.clone()).lines);
                }
                Text::from(lines)
            }
            None => Text::raw(
                "No artifacts on this branch yet.\n\nArtifact actions and put-artifact register documents for later steps and human review.",
            ),
        };
        let title = if app.render_markdown {
            " Artifact · Markdown · ↑/↓ scroll · m raw "
        } else {
            " Artifact · Raw text · ↑/↓ scroll · m Markdown "
        };
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(border(title, app.focus == Focus::Detail)),
            panes[1],
        );
    } else if app.blocker_card && workspace.is_some_and(|w|w.blocker.is_some() || matches!(w.primary,crate::workspace_view::PrimaryAction::RecoverOwner|crate::workspace_view::PrimaryAction::RecoverWrites)) {
        let workspace=workspace.unwrap();
        let mut message=format!("{}\n{}\n\n",workspace.objective,workspace.attempt_label());
        if let Some(blocker)=&workspace.blocker {message.push_str(&format!("{}\n\nEvidence: event #{}\n\n",blocker.reason,blocker.event_id));}
        message.push_str(&format!("Next: {}\n\ne: inspect evidence\nh: activity history\nc: exact context and version references\n",workspace.primary.label()));
        frame.render_widget(Paragraph::new(crate::markdown::render(&safe_display(&message))).wrap(Wrap {trim:false}).scroll((app.detail_scroll,0))
            .block(Block::default().borders(Borders::ALL).title(" Needs attention — evidence before recovery ").border_style(Style::default().fg(Color::Yellow))),columns[1]);
    } else {
        let right=Layout::vertical([Constraint::Length(4),Constraint::Min(3)]).split(columns[1]);
        let context=workspace.map(|w|format!("{}\nOutputs: {} · verification: {}",w.objective,w.outputs.as_ref().map(|v|if v.is_empty(){"none declared".into()}else{v.join(", ")}).unwrap_or_else(||"unknown".into()),w.verification))
            .unwrap_or_else(||app.workspace_error.clone().unwrap_or_else(||"Choose a session".into()));
        frame.render_widget(Paragraph::new(safe_display(&context)).wrap(Wrap {trim:false}).block(border(" Step context · c for exact records ",false)),right[0]);
        let show_detail=app.focus==Focus::Detail || (app.focus==Focus::Compose && app.focus_before_compose==Focus::Detail);
        if show_detail {
            frame.render_widget(Paragraph::new(safe_display(&app.detail())).wrap(Wrap {trim:false}).scroll((app.detail_scroll,0))
                .block(border(if app.diff.is_some(){" Recorded diff · Left returns "}else{" Evidence · Left returns to activity "},true)),right[1]);
        } else {
            let events=app.events.iter().map(|event| {
                let item=ListItem::new(inline_display(&format!("#{} {}: {}",event.id,conversational_label(&event.kind),event.data.lines().next().unwrap_or(""))));
                if matches!(event.kind.as_str(),"event.invalid"|"step.error"|"runner.error"|"workspace.restore-failed"|"verification.failed"|"tool.error"){item.style(Style::default().fg(Color::Red))}else{item}
            });
            let list=List::new(events).block(border(if app.show_all{" Activity · full history "}else{" Activity · Enter opens evidence "},app.focus==Focus::Events))
                .highlight_symbol("▶ ").highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD));
            let mut selected=ListState::default();if !app.events.is_empty(){selected.select(Some(app.event_index));}
            frame.render_stateful_widget(list,right[1],&mut selected);
        }
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
        " Message agent · i or Enter to focus · Enter sends "
    };
    let composer_style = if app.composer.is_empty() {
        Style::default().fg(Color::Rgb(85, 85, 85))
    } else {
        Style::default()
    };
    let composer = Paragraph::new(input)
        .style(composer_style)
        .wrap(Wrap { trim: false })
        .block(border(input_title, app.focus == Focus::Compose));
    frame.render_widget(composer, outer[2]);

    let footer = if app.approval.is_some() {
        "Press a decision key · Esc denies · Enter does nothing · ↑/↓ scroll request".into()
    } else if app.diagnostic.is_some() {
        "Attention required · ↑/↓ scroll full diagnostic · Esc close · inspect evidence before retrying".into()
    } else if app.context_detail.is_some() || app.usage_detail.is_some() {
        "↑/↓ scroll · Esc close".into()
    } else if app.restart_plan.is_some() {
        "Restore uses recorded edits · y confirm / n cancel · ↑/↓ scroll preview".into()
    } else if !app.restart_choices.is_empty() {
        "j/k choose step  ·  Enter preview  ·  Esc cancel".into()
    } else {
        format!(
            "{}   │   1/2 views · ←/→ pane · ↑/↓ select/scroll · Enter open · i message · m raw",
            app.notice
        )
    };
    frame.render_widget(Paragraph::new(format!("{}\n{footer}",app.usage_line)), outer[3]);
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
    if let Err(error) = store.ensure_no_pending_writes() {
        app.notice = format!("Recovery required: {error:#}");
    }
    ratatui::run(|terminal| -> Result<()> {
        let mut last_refresh = Instant::now();
        loop {
            app.tick();
            if last_refresh.elapsed() >= Duration::from_millis(600) {
                app.refresh(&store)?;
                last_refresh = Instant::now();
            }
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(150))? {
                if let Event::Key(key) = event::read()? {
                    match app.on_key(key, &root, &store) {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(error) => app.raise_diagnostic(format!("Action error: {error:#}")),
                    }
                }
            }
        }
    })
}

#[cfg(test)]
#[path = "tui_p0_tests.rs"]
mod p0_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_worker_expires_approval_and_does_not_replay_it() {
        let (prompt_tx, prompts) = std::sync::mpsc::channel();
        let (answers, old_answers) = std::sync::mpsc::channel();
        let (finished_tx, finished) = std::sync::mpsc::channel();
        let mut app = App::new(None);
        app.runner = Some(Runner { prompts, answers, finished, session_id:None });
        app.approval = Some(ApprovalPrompt {
            content: "old request".into(), mode: ApprovalMode::Persistent,
        });
        drop(prompt_tx);
        drop(finished_tx); // A worker panic disconnects without a completion message.
        app.tick();
        assert!(app.runner.is_none());
        assert!(app.approval.is_none());
        assert!(app.notice.contains("pending approval expired"));
        assert!(old_answers.try_recv().is_err());
        let (_prompt_tx, prompts) = std::sync::mpsc::channel();
        let (answers, fresh_answers) = std::sync::mpsc::channel();
        let (_finished_tx, finished) = std::sync::mpsc::channel();
        app.runner = Some(Runner { prompts, answers, finished, session_id:None });
        assert!(app.deliver_approval('y').is_err());
        assert!(fresh_answers.try_recv().is_err());
    }

    #[test]
    fn enter_opens_composer_and_text_is_editable() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("example", "goal", "plan").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        assert_eq!(app.focus, Focus::Events, "opening a session must not auto-focus the composer");
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Detail);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Compose);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), root.path(), &store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), root.path(), &store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.composer, "x");
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Detail);
    }

    #[test]
    fn focus_moves_across_panes_and_arrows_scroll_detail() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("example", "goal", "plan").unwrap();
        store.add_artifact(&session, "plan", "plan.md", "# Heading\n\nLong artifact").unwrap();
        let mut app = App::new(None);
        app.refresh(&store).unwrap();
        assert_eq!(app.focus, Focus::Sessions);
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Events);
        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Artifacts);
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Detail);
        let selected = app.artifact_index;
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), root.path(), &store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.detail_scroll, 2);
        app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.detail_scroll, 2, "PageDown is no longer a scrolling binding");
        assert_eq!(app.artifact_index, selected);
        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.detail_scroll, 1);
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Artifacts);
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Sessions);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Compose);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.focus, Focus::Sessions);
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
    fn approval_shortcuts_submit_immediately_but_enter_does_not() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let (prompts_tx, prompts_rx) = std::sync::mpsc::channel();
        let (answer_tx, answer_rx) = std::sync::mpsc::channel();
        let (_finish_tx, finish_rx) = std::sync::mpsc::channel();
        let mut app = App::new(None);
        app.runner = Some(Runner {
            session_id: None,
            prompts: prompts_rx, answers: answer_tx, finished: finish_rx,
        });
        app.approval = Some(ApprovalPrompt {
            content: "Approve a shell command".into(),
            mode: ApprovalMode::Persistent,
        });
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert!(answer_rx.try_recv().is_err());
        assert!(app.approval.is_some());
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.detail_scroll, 1);
        app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.detail_scroll, 1);
        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(answer_rx.try_recv().unwrap(), "g");
        assert!(app.approval.is_none());
        drop(prompts_tx);
    }

    #[test]
    fn human_approval_rejects_global_scope_and_escape_denies() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let (_prompts_tx, prompts_rx) = std::sync::mpsc::channel();
        let (answer_tx, answer_rx) = std::sync::mpsc::channel();
        let (_finish_tx, finish_rx) = std::sync::mpsc::channel();
        let mut app = App::new(None);
        app.runner = Some(Runner {
            session_id: None,
            prompts: prompts_rx, answers: answer_tx, finished: finish_rx,
        });
        app.approval = Some(ApprovalPrompt {
            content: "Human checkpoint".into(),
            mode: ApprovalMode::Human,
        });
        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert!(answer_rx.try_recv().is_err());
        assert!(app.approval.is_some());
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(answer_rx.try_recv().unwrap(), "n");
        app.approval = Some(ApprovalPrompt {
            content: "Another human checkpoint".into(),
            mode: ApprovalMode::Human,
        });
        app.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(answer_rx.try_recv().unwrap(), "y");
    }

    #[test]
    fn artifact_tab_displays_latest_branch_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("example", "goal", "plan").unwrap();
        store.add_artifact(&session, "plan", "plan.md", "first version").unwrap();
        store.add_artifact(&session, "plan", "plan.md", "revised version").unwrap();
        store.add_artifact(&session, "plan", "notes.md", "Notes").unwrap();

        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.view, View::Artifacts);
        assert_eq!(app.focus, Focus::Artifacts);
        assert_eq!(app.artifacts.len(), 2);
        let plan_index = app.artifacts.iter().position(|a| a.name == "plan.md").unwrap();
        app.artifact_index = plan_index;
        assert_eq!(app.artifact().unwrap().content, "revised version");
        store.add_artifact(&session, "plan", "plan.md", "third version").unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.artifact().unwrap().name, "plan.md");
        assert_eq!(app.artifact().unwrap().content, "third version");
        assert!(app.render_markdown);
        app.detail_scroll = 10;
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert!(!app.render_markdown);
        assert_eq!(app.detail_scroll, 0);
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert!(app.render_markdown);
        let backend = ratatui::backend::TestBackend::new(110, 32);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE), root.path(), &store).unwrap();
        assert_eq!(app.view, View::Conversation);
        assert_eq!(app.focus, Focus::Events);
    }

    #[test]
    fn artifact_tab_handles_no_saved_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        store.create("example", "goal", "plan").unwrap();
        let mut app = App::new(None);
        app.refresh(&store).unwrap();
        app.switch_view(View::Artifacts);
        let backend = ratatui::backend::TestBackend::new(100, 26);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(app.artifact().is_none());
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
        store.event(&session, Some("explore"), "permission.decision", "write: approved once").unwrap();
        store.event(&session, Some("explore"), "tool.error", "user denied write").unwrap();
        store.event(&session, Some("explore"), "human.approved", "").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        assert!(app.events.iter().any(|e| e.kind == "protocol.error"));
        assert!(app.events.iter().all(|e| e.kind != "model.output"));
        assert!(app.events.iter().all(|e| e.kind != "permission.decision"));
        assert!(app.events.iter().any(|e| e.kind == "tool.error"));
        assert!(app.events.iter().any(|e| e.kind == "human.approved"));
        app.toggle_verbose();
        app.refresh(&store).unwrap();
        assert!(app.events.iter().any(|e| e.kind == "model.output"));
        assert!(app.events.iter().any(|e| e.kind == "permission.decision"));
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
