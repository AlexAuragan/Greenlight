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
mod activity;
mod shell;
mod theme;
mod edits;
mod result;
mod editor;
mod approval;
mod focus;
mod recovery;
mod routing;
mod view;
mod help;
mod reader;
mod reading;
mod exports;
mod manager_ui;
#[cfg(test)]
#[path="tui_reading_tests.rs"]
mod reading_tests;
#[cfg(test)]
#[path="tui_manager_tests.rs"]
mod manager_tests;
use activity::{is_chatter,conversational_label,invalid_event_payload};
use help::HELP;
use view::draw;

use std::{
    cell::Cell,
    collections::HashMap,
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
    Edits,
}

struct App {
    shell: shell::Shell,
    inspector_open: bool,
    edits: edits::Edits,
    result_open: bool,
    result_text: String,
    result_overview: Option<result::Overview>,
    result_key: Option<(String,String,i64)>,
    sessions: Vec<Session>,
    session_index: usize,
    events: Vec<std::sync::Arc<EventRow>>,
    activity_rows:Vec<std::sync::Arc<EventRow>>,
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
    composer_editor: editor::Editor,
    goal_editor: editor::Editor,
    session_editor: editor::Editor,
    filter_editor: editor::Editor,
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
    session_query: String,
    searching: bool,
    session_total: usize,
    session_activity: crate::session_picker::Activity,
    new_session: Option<crate::session_picker::NewSession>,
    drafts: HashMap<Option<String>, String>,
    composer_session: Option<String>,
    follow_activity: bool,
    help: bool,
    rendered_scroll: Cell<Option<(u16,u16)>>,
    viewport: (u16,u16),
    artifact_browser: Option<crate::artifact_history::Browser>,
    reader:reader::Reader,
    activity_cards:crate::activity_cards::Projection,
    activity_generation:Option<(String,String,Option<i64>)>,
    reading:reading::Reading,
    activity_query:String,
    activity_filtering:bool,
    copy_job:Option<std::sync::mpsc::Receiver<Result<String,String>>>,
    pending_editor:Option<exports::Snapshot>,
    manager_job:Option<manager_ui::ManagerJob>,
    manager_enabled:bool,
}

impl App {
    fn new(initial_session: Option<&str>) -> Self {
        Self {
            shell: Default::default(),
            inspector_open: false,
            edits: Default::default(),
            result_open: false,
            result_text: String::new(),
            result_overview: None,
            result_key: None,
            sessions: Vec::new(),
            session_index: 0,
            events: Vec::new(),
            activity_rows:Vec::new(),
            event_index: 0,
            artifacts: Vec::new(),
            artifact_index: 0,
            view: View::Conversation,
            render_markdown: true,
            focus: Focus::Events,
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
            composer_editor: Default::default(),
            goal_editor: Default::default(),
            session_editor: Default::default(),
            filter_editor: Default::default(),
            notice: String::new(),
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
            session_query: String::new(),
            searching: false,
            session_total: 0,
            session_activity: Default::default(),
            new_session: None,
            drafts: HashMap::new(),
            composer_session: None,
            follow_activity: true,
            help: false,
            rendered_scroll: Cell::new(None),
            viewport: (80,24),
            artifact_browser: None,
            reader:reader::Reader::default(),
            activity_cards:crate::activity_cards::Projection::default(),
            activity_generation:None,
            reading:reading::Reading::default(),
            activity_query:String::new(),activity_filtering:false,
            copy_job:None,pending_editor:None,
            manager_job:None,manager_enabled:false,
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
        self.events.get(self.event_index).map(std::sync::Arc::as_ref)
    }

    fn artifact(&self) -> Option<&Artifact> {
        self.artifacts.get(self.artifact_index)
    }










    fn refresh(&mut self, store: &Store) -> Result<()> {
        if let Some(root)=store.directory.parent().and_then(Path::parent) {
            self.manager_enabled=crate::config::load_config(root).map(|c|c.manager.enabled).unwrap_or(false);
        }
        self.remember_reading();
        let old_session = self.session().map(|s| s.id.clone());
        let old_event = self.event().map(|e| e.id);
        let old_artifact = self.artifact().map(|a| a.name.clone());
        let following_tail = self.follow_activity;

        self.sessions = store.list()?;
        self.session_total=self.sessions.len();
        self.session_activity=store.session_activity()?;
        crate::session_picker::filter_and_sort(&mut self.sessions,&self.session_query,&self.session_activity);
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
        let loaded_session_changed=self.workspace_session.as_deref()!=self.session().map(|s|s.id.as_str());
        self.sync_draft();
        let reading_scope_changed=self.reading_scope();
        let old_artifact_version=self.artifact().map(|a|a.event_id);
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
            if session_changed {self.context_detail=None;self.follow_activity=true;}
            if session.status == "completed" {
                let completion = history.iter().rev().find(|e|e.2=="session.completed").map(|e|e.0).unwrap_or(0);
                let key=(session.id.clone(),session.active_branch.clone(),completion);
                if self.result_key.as_ref()!=Some(&key) {
                    self.result_open=true;self.detail_scroll=0;self.result_key=Some(key);
                }
                self.result_text=result::summary(store,&session,&history);
                self.result_overview=Some(result::overview(&session,&history));
            } else {self.result_open=false;self.result_key=None;self.result_overview=None;}
            let show_all = self.show_all;
            let generation=(session.id.clone(),session.active_branch.clone(),history.last().map(|event|event.0));
            if self.activity_generation.as_ref()!=Some(&generation) {
                self.activity_cards=crate::activity_cards::Projection::build(&history);
                self.activity_generation=Some(generation);
            }
            self.activity_rows = history
                .into_iter()
                .map(|(id, step, kind, data)| {
                    if let Some(error) = self.activity_cards.malformed.get(&id).cloned().or_else(||invalid_event_payload(&kind,&data)) {
                        EventRow {id,step,kind:"event.invalid".into(),data:format!("Malformed {kind} event #{id}: {error}\n\nRaw evidence:\n{data}")}
                    } else { EventRow {id,step,kind,data} }
                })
                .filter(|event| show_all || !is_chatter(&event.kind))
                .map(std::sync::Arc::new)
                .collect();
            self.filter_activity();
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
            self.events.clear();self.activity_rows.clear();
            self.activity_cards=crate::activity_cards::Projection::default();self.activity_generation=None;
            self.artifacts.clear();
            self.diagnostic=None;self.usage_detail=None;self.usage_error=None;
            self.usage_line="Usage: no session selected".into();
            self.workspace=None;self.workspace_error=None;self.workspace_session=None;self.blocker_card=false;self.context_detail=None;
        }
        if self.view==View::Edits && self.edits.pending.is_none() {self.refresh_edits(store);}
        if self.session().is_none(){self.result_open=false;self.result_key=None;}
        if changed_session {
            self.artifact_index = 0;
        } else if let Some(name) = old_artifact {
            self.artifact_index = self.artifacts.iter()
                .position(|artifact| artifact.name == name)
                .unwrap_or(0);
        } else {
            self.artifact_index = 0;
        }
        if changed_session || loaded_session_changed || following_tail {
            self.event_index = self.events.len().saturating_sub(1);
        } else if let Some(id) = old_event {
            self.event_index=self.events.iter().position(|event|event.id==id).unwrap_or(self.events.len().saturating_sub(1));
        } else {self.event_index=self.events.len().saturating_sub(1);}
        let event_changed=old_event!=self.event().map(|e|e.id);
        if (changed_session || event_changed) && self.view==View::Conversation && self.diagnostic.is_none() && self.approval.is_none() && self.usage_detail.is_none() && self.context_detail.is_none() && !self.help && !self.blocker_card && !self.result_open {
            self.detail_scroll=0;
        }
        if let Some(mut browser)=self.artifact_browser.take() {
            if let Some(session)=self.session() {
                if browser.session==session.id && browser.branch==session.active_branch {
                    match browser.refresh(store,session) {Ok(())=>self.artifact_browser=Some(browser),Err(error)=>self.notice=format!("Version browser closed: {error:#}")}
                } else {self.notice="Version browser closed because session/branch changed.".into();}
            }
        }
        if self.diff.as_ref().is_some_and(|(id, _)| self.event().map(|event| event.id) != Some(*id)) {
            self.diff = None;
        }
        if reading_scope_changed {self.restore_reading();}
        else if self.view==View::Artifacts && old_artifact_version!=self.artifact().map(|a|a.event_id) && !self.reading_overlay(){self.detail_scroll=0;}
        Ok(())
    }






    fn tick(&mut self) {
        self.remember_reading();
        self.poll_copy();
        self.poll_manager();
        let Some(runner) = &self.runner else { return };
        if self.approval.is_none() {
            match runner.prompts.try_recv() {
                Ok(question) => {
                    self.activity_filtering=false;
                    self.artifact_browser=None;
                    self.help=false;
                    self.searching=false;
                    self.session_query.clear();
                    self.new_session=None;
                    if let Some(id)=runner.session_id.as_ref() {
                        if let Some(index)=self.sessions.iter().position(|s|&s.id==id) {
                            self.session_index=index;
                            if self.workspace_session.as_deref()!=Some(id) {self.workspace=None;self.context_detail=None;}
                        }
                        self.requested_session=Some(id.clone());
                    }
                    self.shell.request_epoch += 1;
                    self.shell.actions_focus = false;
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
                Ok(()) => "Workflow finished. Review the result and recorded evidence.".into(),
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
        let original=self.composer.clone();
        let instruction = self.composer.trim().to_string();
        if instruction.is_empty() {
            return;
        }
        if let Some(text)=original.strip_prefix("/ask ") {self.start_manager(root,store,text.to_owned());return;}
        if instruction=="/ask" {self.notice="Use /ask QUESTION for read-only manager inference; /manager-context previews shared context.".into();return;}
        if instruction=="/cancel-manager" {self.cancel_manager();self.composer.clear();return;}
        if instruction=="/manager" {
            self.view=View::Conversation;self.activity_query="manager.".into();self.filter_activity();self.commit_activity_filter();self.composer.clear();self.focus=Focus::Events;self.follow_activity=false;return;
        }
        if instruction=="/manager-context" {
            if let Some(session)=self.session() {
                let artifacts=Vec::new();
                match crate::manager::capture(root,store,&session.id,"preview",&artifacts) {
                    Ok(snapshot)=>{self.context_detail=Some(format!("Manager context preview · no inference\n{}",serde_json::to_string_pretty(&snapshot).unwrap_or_default()));self.detail_scroll=0;}
                    Err(error)=>self.raise_diagnostic(format!("Manager context unavailable: {error:#}")),
                }
            }
            self.composer.clear();return;
        }
        if instruction == "/restart" {
            self.composer.clear();
            self.open_restart(root, store);
            return;
        }
        if instruction == "/events" {
            self.toggle_verbose();
            self.composer.clear();
            return;
        }
        if instruction == "/help" {
            self.open_help();self.composer.clear();return;
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
        if instruction!="/resume" && self.manager_enabled {self.start_manager(root,store,original);return;}
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




}


fn move_index(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    current.saturating_add_signed(delta).min(len - 1)
}

fn border(title: &'static str, focused: bool) -> Block<'static> {
    let color = if focused { theme::FOCUS } else { theme::BORDER };
    Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(color))
}
fn safe_display(text:&str)->String {text.chars().filter(|c|!c.is_control() || *c=='\n' || *c=='\t').collect()}
fn inline_display(text:&str)->String {safe_display(text).replace(['\n','\t']," ")}





fn draw_scrolled(frame:&mut Frame,app:&App,paragraph:Paragraph<'_>,area:ratatui::layout::Rect) {
    if area.width==0 || area.height==0 {return;}
    let max=paragraph.line_count(area.width.saturating_sub(2)).saturating_sub(area.height as usize).min(u16::MAX as usize) as u16;
    let offset=app.detail_scroll.min(max);
    app.rendered_scroll.set(Some((app.detail_scroll,offset)));
    frame.render_widget(paragraph.scroll((offset,0)),area);
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
    app.shell.project=root.file_name().map(|name|name.to_string_lossy().into_owned()).unwrap_or_else(||"project".into());
    app.refresh(&store)?;
    if let Err(error) = store.ensure_no_pending_writes() {
        app.notice = format!("Recovery required: {error:#}");
    }
    let result = ratatui::run(|terminal| -> Result<()> {
        crossterm::execute!(io::stdout(), crossterm::cursor::SetCursorStyle::BlinkingBar)?;
        let _capture = shell::CaptureGuard::start()?;
        let mut captured_mouse = true;
        let mut last_refresh = Instant::now();
        loop {
            let size=terminal.size()?;
            app.viewport=(size.width,size.height);
            app.tick();
            if captured_mouse != app.shell.mouse {shell::set_mouse(app.shell.mouse)?;captured_mouse=app.shell.mouse;}
            if let Some(snapshot)=app.pending_editor.take(){
                crossterm::execute!(io::stdout(), event::DisableMouseCapture, event::DisableBracketedPaste)?;
                let result=exports::with_terminal_suspended(||exports::open_editor(&snapshot))?;
                crossterm::execute!(io::stdout(), event::EnableBracketedPaste)?;
                shell::set_mouse(app.shell.mouse)?;
                captured_mouse=app.shell.mouse;
                terminal.clear()?;
                match result{Ok(())=>app.notice="Editor closed; snapshot changes were not imported.".into(),Err(error)=>app.raise_diagnostic(format!("Editor unavailable: {error:#}"))}
            }
            if app.requested_session.is_some() || app.composer_session.as_deref()!=app.session().map(|s|s.id.as_str()) || last_refresh.elapsed() >= Duration::from_millis(600) {
                app.refresh(&store)?;
                last_refresh = Instant::now();
            }
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(150))? {
                {
                    let event = event::read()?;
                    match app.on_event(event, &root, &store) {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(error) => app.raise_diagnostic(format!("Action error: {error:#}")),
                    }
                }
            }
        }
    });
    crossterm::execute!(io::stdout(), crossterm::cursor::SetCursorStyle::DefaultUserShape)?;
    result
}

#[cfg(test)]
#[path = "tui_p0_tests.rs"]
mod p0_tests;

#[cfg(test)]
#[path = "tui_picker_tests.rs"]
mod picker_tests;

#[cfg(test)]
#[path = "tui_navigation_tests.rs"]
mod navigation_tests;

#[cfg(test)]
#[path = "tui_tests.rs"]
mod tests;

#[cfg(test)]
#[path="tui_edits_tests.rs"]
mod edits_tests;

#[cfg(test)]
#[path = "tui_presentation_tests.rs"]
mod presentation_tests;
