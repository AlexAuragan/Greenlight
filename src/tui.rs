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
mod actions;
mod activity;
mod approval;
mod artifact_review;
mod attention;
mod checkpoint;
mod compose;
mod details;
mod editor;
mod edits;
mod exports;
mod focus;
mod help;
#[cfg(test)]
#[path = "tui_manager_tests.rs"]
mod manager_tests;
mod manager_ui;
mod navigation;
mod reader;
mod reading;
#[cfg(test)]
#[path = "tui_reading_tests.rs"]
mod reading_tests;
mod recovery;
mod result;
mod routing;
mod shell;
mod theme;
mod view;
use activity::{conversational_label, invalid_event_payload, is_chatter};
use help::text as help_text;
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

#[derive(PartialEq, Eq)]
struct ActivityGeneration {
    history_cutoff: Option<i64>,
    session: String,
    branch: String,
    revision: (u64, i64),
    show_all: bool,
}
struct App {
    project: Option<crate::project_execution::ProjectConflict>,
    shell: shell::Shell,
    inspector_open: bool,
    edits: edits::Edits,
    compose: compose::Composer,
    result_open: bool,
    saved_workflow_request: bool,
    saved_manager_request: bool,
    result_text: String,
    result_overview: Option<result::Overview>,
    result_key: Option<(String, String, i64)>,
    sessions: Vec<Session>,
    session_index: usize,
    navigation: navigation::Navigation,
    events: Vec<std::sync::Arc<EventRow>>,
    activity_rows: Vec<std::sync::Arc<EventRow>>,
    event_index: usize,
    rerun_output: Option<(String, String)>,
    artifacts: Vec<crate::store::ArtifactMetadata>,
    artifact_generation: Option<(String, String, (u64, i64))>,
    artifact_body: Option<Artifact>,
    artifact_body_key: Option<(String, String, i64)>,
    artifact_error: Option<String>,
    partial_history: bool,
    exact_event: Option<std::sync::Arc<EventRow>>,
    preview_generation: Option<(String, String, (u64, i64))>,
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
    approval_hidden: bool,
    approval_return_scroll: u16,
    checkpoint: checkpoint::State,
    restart_choices: Vec<RestartCandidate>,
    restart_cursor: usize,
    restart_plan: Option<RestorePlan>,
    restart_route: Option<(crate::manager_delivery::Route, String)>,
    restart_preview: String,
    restart_branch: Option<String>,
    diagnostic: Option<String>,
    diagnostic_detail: bool,
    diagnostic_revision: u64,
    diagnostic_attention: bool,
    attention: Option<attention::State>,
    attention_return: bool,
    diagnostic_key: Option<(String, String)>,
    artifact_diagnostic_key: Option<(String, String)>,
    details: details::Details,
    usage_line: String,
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
    drafts: HashMap<compose::Scope, String>,
    composer_session: Option<String>,
    follow_activity: bool,
    help: bool,
    rendered_scroll: Cell<Option<(u16, u16)>>,
    viewport: (u16, u16),
    artifact_browser: Option<crate::artifact_history::Browser>,
    reader: reader::Reader,
    activity_cards: crate::activity_cards::Projection,
    activity_generation: Option<ActivityGeneration>,
    reading: reading::Reading,
    activity_query: String,
    activity_filtering: bool,
    copy_job: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    pending_editor: Option<exports::Snapshot>,
    pending_artifact_review: Option<artifact_review::Job>,
    manager_job: Option<manager_ui::ManagerJob>,
    manager_enabled: bool,
}

impl App {
    fn new(initial_session: Option<&str>) -> Self {
        Self {
            shell: Default::default(),
            project: None,
            inspector_open: false,
            edits: Default::default(),
            compose: Default::default(),
            result_open: false,
            saved_workflow_request: false,
            saved_manager_request: false,
            result_text: String::new(),
            result_overview: None,
            result_key: None,
            sessions: Vec::new(),
            session_index: 0,
            navigation: navigation::Navigation::default(),
            events: Vec::new(),
            activity_rows: Vec::new(),
            event_index: 0,
            rerun_output: None,
            artifacts: Vec::new(),
            artifact_generation: None,
            artifact_body: None,
            artifact_body_key: None,
            artifact_error: None,
            partial_history: false,
            exact_event: None,
            preview_generation: None,
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
            approval_hidden: false,
            approval_return_scroll: 0,
            checkpoint: checkpoint::State::default(),
            restart_choices: Vec::new(),
            restart_cursor: 0,
            restart_plan: None,
            restart_route: None,
            restart_preview: String::new(),
            restart_branch: None,
            diagnostic: None,
            diagnostic_detail: false,
            diagnostic_revision: 0,
            diagnostic_attention: false,
            attention: None,
            attention_return: false,
            diagnostic_key: None,
            artifact_diagnostic_key: None,
            details: details::Details::default(),
            usage_line: "Usage: no recorded model requests".into(),
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
            viewport: (80, 24),
            artifact_browser: None,
            reader: reader::Reader::default(),
            activity_cards: crate::activity_cards::Projection::default(),
            activity_generation: None,
            reading: reading::Reading::default(),
            activity_query: String::new(),
            activity_filtering: false,
            copy_job: None,
            pending_editor: None,
            pending_artifact_review: None,
            manager_job: None,
            manager_enabled: false,
        }
    }
    fn raise_diagnostic(&mut self, message: String) {
        self.diagnostic_revision = self.diagnostic_revision.wrapping_add(1);
        self.diagnostic_detail = false;
        self.diagnostic_attention = false;
        self.notice = format!(
            "Attention required: {}",
            message.lines().next().unwrap_or("inspect diagnostic")
        );
        self.diagnostic = Some(message);
        self.detail_scroll = 0;
    }

    fn session(&self) -> Option<&Session> {
        self.sessions.get(self.session_index)
    }

    fn event(&self) -> Option<&EventRow> {
        let selected = self.events.get(self.event_index)?;
        Some(
            self.exact_event
                .as_ref()
                .filter(|e| self.partial_history && e.id == selected.id)
                .unwrap_or(selected)
                .as_ref(),
        )
    }

    fn artifact(&self) -> Option<&Artifact> {
        self.artifact_body.as_ref().filter(|body| {
            self.artifacts
                .get(self.artifact_index)
                .is_some_and(|m| m.event_id == body.event_id)
        })
    }
    fn load_selected_artifact(&mut self, store: &Store) {
        let selected = self.artifacts.get(self.artifact_index).cloned();
        let key = self
            .session()
            .zip(selected.as_ref())
            .map(|(s, m)| (s.id.clone(), s.active_branch.clone(), m.event_id));
        if key == self.artifact_body_key {
            return;
        }
        self.artifact_body_key = key;
        self.artifact_body = None;
        self.artifact_error = None;
        if let Some(meta) = selected {
            match store.read_blob(&meta.blob) {
                Ok(content) => self.artifact_body = Some(Artifact {name:meta.name.clone(),step:meta.step.clone(),event_id:meta.event_id,content}),
                Err(error) => self.artifact_error = Some(format!("Artifact body unavailable: {error:#}. The saved body was not truncated. Check limits.blob_bytes and restart after changing it.")),
            }
        }
    }

    fn load_selected_event(&mut self, store: &Store) -> Result<()> {
        if !self.partial_history {
            return Ok(());
        }
        if self
            .event()
            .zip(self.exact_event.as_ref())
            .is_some_and(|(a, b)| a.id == b.id)
        {
            return Ok(());
        }
        if let Some(event) = self.event().cloned() {
            let session = self.session().context("No session selected")?;
            let data = store.event_body(session, event.id)?;
            self.exact_event = Some(std::sync::Arc::new(EventRow { data, ..event }));
        }
        Ok(())
    }
    fn project_attention(&self) -> bool {
        self.other_project_owner() || self.project.as_ref().is_some_and(|p| p.interrupted)
    }
    fn other_project_owner(&self) -> bool {
        self.project.as_ref().is_some_and(|p| {
            p.session
                .as_ref()
                .is_none_or(|s| self.session().is_none_or(|selected| selected.id != s.id))
        })
    }
    fn refresh(&mut self, store: &Store) -> Result<()> {
        self.project = match store.project_status() {
            Ok(project) => project,
            Err(error) => {
                self.notice = format!("Project ownership unavailable: {error:#}. Execution remains guarded; inspect state before retrying.");
                None
            }
        };
        self.saved_workflow_request = false;
        self.saved_manager_request = false;
        if let Some(root) = store.directory.parent().and_then(Path::parent) {
            let config = crate::config::load_config(root);
            self.details.configured_model = config.as_ref().ok().map(|c| c.provider.model.clone());
            self.details.budget_config = config.as_ref().ok().map(|c| c.budgets.clone());
            self.details.model_roles = config.as_ref().ok().map(|c| c.model_roles.clone());
            self.details.provider = config
                .as_ref()
                .ok()
                .map(|c| crate::provider::ConnectionInfo::current(&c.provider));
            self.manager_enabled = config.map(|c| c.manager.enabled).unwrap_or(false);
        }
        self.remember_reading();
        let old_session = self.session().map(|s| s.id.clone());
        let old_event = self.event().map(|e| e.id);
        let old_artifact = self.artifact().map(|a| a.name.clone());
        let following_tail = self.follow_activity;

        match store
            .list()
            .and_then(|sessions| Ok((sessions, store.session_activity()?)))
        {
            Ok((sessions, activity)) => {
                self.sessions = sessions;
                self.session_activity = activity;
                self.navigation.error = None;
            }
            Err(error) => {
                self.sessions.clear();
                self.session_activity.clear();
                self.navigation.error = Some(format!("Session discovery unavailable: {error:#}"));
            }
        }
        self.navigation.load_error = None;
        self.session_total = self.sessions.len();
        crate::session_picker::filter_and_sort(&mut self.sessions, "", &self.session_activity);
        let desired_session = self
            .requested_session
            .take()
            .or(old_session.clone())
            .or(self.navigation.opened.clone());
        self.session_index = if let Some(id) = desired_session {
            self.navigation.opened = Some(id.clone());
            self.sessions
                .iter()
                .position(|s| s.id == id)
                .unwrap_or(usize::MAX)
        } else {
            0
        };
        if let Some(s) = self.sessions.get(self.session_index) {
            self.navigation.opened = Some(s.id.clone());
        }
        self.navigation.missing = if self.navigation.error.is_none()
            && self.navigation.opened.is_some()
            && self.sessions.get(self.session_index).is_none()
        {
            Some("Opened session is no longer available. Choose another session explicitly; no replacement was opened.".into())
        } else {
            None
        };
        self.navigation
            .refresh(&self.sessions, &self.session_query, &self.session_activity);
        self.navigation.setup = None;
        if self.session_total == 0 && self.navigation.error.is_none() {
            if let Some(root) = store.directory.parent().and_then(Path::parent) {
                self.navigation.setup=match crate::config::load_config(root) {
                    Err(_)=>Some("Project configuration unavailable. Run greenlight init or inspect the project configuration before starting.".into()),
                    Ok(_)=>match crate::session_picker::NewSession::discover(root){Ok(f) if f.workflows.is_empty()=>Some("No workflows available. Run greenlight init or add a workflow before starting.".into()),Ok(f) if f.workflows.iter().all(|w|w.error.is_some())=>Some("No valid workflows available. Inspect workflow definitions before starting.".into()),Err(e)=>Some(format!("Workflow discovery unavailable: {e:#}")),_=>None},
                };
            }
        }
        let loaded_session_changed =
            self.workspace_session.as_deref() != self.session().map(|s| s.id.as_str());
        self.sync_draft();
        let reading_scope_changed = self.reading_scope();
        let old_artifact_version = self.artifact().map(|a| a.event_id);
        let changed_session = old_session.as_deref() != self.session().map(|s| s.id.as_str());
        if let Some(session) = self.session().cloned() {
            let preview_generation = (
                session.id.clone(),
                session.active_branch.clone(),
                store.data_revision()?,
            );
            // Capture before loading history or derived views. A commit during
            // this refresh must invalidate the next one, not tag old rows as new.
            let revision = preview_generation.2;
            if self.partial_history && self.preview_generation.as_ref() == Some(&preview_generation)
            {
                self.load_selected_artifact(store);
                return Ok(());
            }
            let history = match store.history(&session) {
                Ok(h) => h,
                Err(e) => {
                    if e.downcast_ref::<crate::resource_limits::Exceeded>()
                        .is_some()
                    {
                        self.exact_event = None;
                        self.partial_history = true;
                        self.preview_generation = Some(preview_generation);
                        self.notice = format!("{e:#} Showing recent previews only.");
                        self.workspace = None;
                        self.workspace_session = Some(session.id.clone());
                        self.blocker_card = false;
                        self.attention = None;
                        self.workspace_error = Some(self.notice.clone());
                        self.result_open = false;
                        self.details.close();
                        self.artifact_browser = None;
                        self.context_detail = None;
                        self.activity_cards = crate::activity_cards::Projection::default();
                        self.activity_generation = None;
                        self.activity_rows = store
                            .history_previews(&session)?
                            .into_iter()
                            .map(|(id, step, kind, data)| {
                                std::sync::Arc::new(EventRow {
                                    id,
                                    step,
                                    kind,
                                    data,
                                })
                            })
                            .collect();
                        self.filter_activity();
                        self.event_index =
                            self.event_index.min(self.events.len().saturating_sub(1));
                        self.artifacts = store.artifact_metadata(&session, "session")?;
                        self.artifact_index = self
                            .artifact_index
                            .min(self.artifacts.len().saturating_sub(1));
                        self.load_selected_artifact(store);
                        self.usage_line =
                            "Full history exceeds budget · recent previews only".into();
                        self.details.report = None;
                        self.edits = edits::Edits::default();
                        return Ok(());
                    }
                    self.navigation.load_error =
                        Some(format!("Opened session history unavailable: {e:#}"));
                    self.clear_diagnostic();
                    self.workspace = None;
                    self.events.clear();
                    self.activity_rows.clear();
                    self.artifacts.clear();
                    self.result_open = false;
                    self.details.close();
                    self.artifact_browser = None;
                    self.context_detail = None;
                    self.edits = edits::Edits::default();
                    return Ok(());
                }
            };
            self.partial_history = false;
            self.exact_event = None;
            self.preview_generation = None;
            for (manager, pending) in [
                (
                    false,
                    crate::continuation::pending_reference(&history, false),
                ),
                (true, crate::continuation::pending_reference(&history, true)),
            ] {
                match pending {
                    Ok(reference) if manager => self.saved_manager_request = reference.is_some(),
                    Ok(reference) => self.saved_workflow_request = reference.is_some(),
                    Err(error) => self.notice = format!("Saved request metadata unreadable ({}): {error}. Recorded history remains available; this saved retry will not start.", if manager {"manager"} else {"workflow"}),
                }
            }

            let session_changed = self.workspace_session.as_deref() != Some(session.id.as_str());
            let old_primary = self.workspace.as_ref().map(|w| w.primary);
            let old_blocker = self
                .workspace
                .as_ref()
                .and_then(|w| w.blocker.as_ref())
                .map(|b| b.event_id);
            self.workspace_session = Some(session.id.clone());
            let local = self
                .runner
                .as_ref()
                .is_some_and(|r| r.session_id.as_deref() == Some(session.id.as_str()));
            match crate::workspace_view::SessionWorkspace::load(store, &session, &history) {
                Ok(workspace) => {
                    let new_blocker = workspace.blocker.as_ref().map(|b| b.event_id);
                    if session_changed
                        || new_blocker != old_blocker
                        || old_primary != Some(workspace.primary)
                    {
                        self.blocker_card = new_blocker.is_some()
                            || matches!(
                                workspace.primary,
                                crate::workspace_view::PrimaryAction::RecoverOwner
                                    | crate::workspace_view::PrimaryAction::RecoverWrites
                            )
                            || workspace.primary == crate::workspace_view::PrimaryAction::Observe
                                && !local;
                    }
                    self.workspace = Some(workspace);
                    self.workspace_error = None;
                }
                Err(error) => {
                    self.workspace = None;
                    self.workspace_error =
                        Some(format!("Workspace projection unavailable: {error:#}"));
                }
            }
            let unchanged = self.activity_generation.as_ref().is_some_and(|g| {
                g.session == session.id
                    && g.branch == session.active_branch
                    && g.revision == revision
                    && g.history_cutoff == history.last().map(|e| e.0)
            });
            let usage = if unchanged && self.details.report.is_some() {
                Ok(self.details.report.take().unwrap())
            } else {
                store.usage_report(&session, crate::usage::Scope::Session)
            };
            self.attention = self.workspace.as_ref().and_then(|workspace| {
                attention::project(
                    &session,
                    workspace,
                    &history,
                    local,
                    usage
                        .as_ref()
                        .ok()
                        .map(|r| r.calls.as_slice())
                        .unwrap_or(&[]),
                )
            });
            if session_changed {
                self.context_detail = None;
                self.follow_activity = true;
            }
            if session.status == "completed" {
                let completion = history
                    .iter()
                    .rev()
                    .find(|e| e.2 == "session.completed")
                    .map(|e| e.0)
                    .unwrap_or(0);
                let key = (
                    session.id.clone(),
                    session.active_branch.clone(),
                    completion,
                );
                if self.result_key.as_ref() != Some(&key) {
                    self.result_open = true;
                    if self.details.page.is_none() {
                        self.detail_scroll = 0;
                    } else {
                        self.details.return_scroll = 0;
                    }
                    self.result_key = Some(key);
                }
                if self.activity_generation.as_ref().is_none_or(|g| {
                    g.session != session.id
                        || g.branch != session.active_branch
                        || g.revision != revision
                        || g.history_cutoff != history.last().map(|e| e.0)
                }) {
                    self.result_text = result::summary(store, &session, &history);
                    self.result_overview = Some(result::overview(&session, &history));
                }
            } else {
                self.result_open = false;
                self.result_key = None;
                self.result_overview = None;
            }
            let show_all = self.show_all;
            let generation = ActivityGeneration {
                history_cutoff: history.last().map(|e| e.0),
                session: session.id.clone(),
                branch: session.active_branch.clone(),
                revision,
                show_all: self.show_all,
            };
            if self.activity_generation.as_ref() != Some(&generation) {
                self.activity_cards = crate::activity_cards::Projection::build(&history);
                self.activity_generation = Some(generation);
                self.activity_rows = history
                    .into_iter()
                    .map(|(id, step, kind, data)| {
                        if let Some(error) = self
                            .activity_cards
                            .malformed
                            .get(&id)
                            .cloned()
                            .or_else(|| invalid_event_payload(&kind, &data))
                        {
                            EventRow {
                                id,
                                step,
                                kind: "event.invalid".into(),
                                data: format!(
                                "Malformed {kind} event #{id}: {error}\n\nRaw evidence:\n{data}"
                            ),
                            }
                        } else {
                            EventRow {
                                id,
                                step,
                                kind,
                                data,
                            }
                        }
                    })
                    .filter(|event| show_all || !is_chatter(&event.kind))
                    .map(std::sync::Arc::new)
                    .collect();
                self.filter_activity();
            }
            if session_changed {
                self.diagnostic = None;
                self.details.close();
            }
            match usage {
                Ok(report) => {
                    self.usage_line = report.totals.compact();
                    self.usage_error = None;
                    self.details.report = Some(report);
                }
                Err(error) => {
                    self.details.report = None;
                    self.usage_line = "Usage unavailable — inspect accounting records".into();
                    let message = format!("Usage accounting could not be read: {error:#}");
                    if self.usage_error.as_ref() != Some(&message) {
                        self.usage_error = Some(message.clone());
                        self.raise_diagnostic(message);
                    }
                }
            }
            let last_restore_outcome = self
                .events
                .iter()
                .rev()
                .find(|e| {
                    matches!(
                        e.kind.as_str(),
                        "workspace.restore-failed" | "workspace.restore-completed"
                    )
                })
                .map(|e| e.id);
            if let Some(event) = self.events.iter().rev().find(|e| {
                e.kind == "event.invalid"
                    || (e.kind == "workspace.restore-failed" && Some(e.id) == last_restore_outcome)
                    || (session.status == "blocked"
                        && matches!(
                            e.kind.as_str(),
                            "step.blocked" | "runner.error" | "step.error"
                        ))
            }) {
                let key = (session.id.clone(), format!("event:{}", event.id));
                if self.diagnostic_key.as_ref() != Some(&key) {
                    let message = format!("{} — event #{}\n\n{}\n\nInspect evidence before retrying. Esc closes this diagnostic.",conversational_label(&event.kind),event.id,event.data);
                    self.diagnostic_key = Some(key);
                    let from_attention = event.kind != "event.invalid"
                        && self.attention.as_ref().is_some_and(|state| {
                            state.evidence == Some(event.id)
                                || self
                                    .workspace
                                    .as_ref()
                                    .and_then(|w| w.blocker.as_ref())
                                    .is_some_and(|b| b.event_id == event.id)
                        });
                    self.raise_diagnostic(message);
                    self.diagnostic_attention = from_attention;
                    if from_attention {
                        self.notice =
                            "Workflow needs attention. Review the evidence before continuing."
                                .into();
                    }
                }
            }
            let artifact_generation = (session.id.clone(), session.active_branch.clone(), revision);
            if self.artifact_generation.as_ref() != Some(&artifact_generation) {
                self.artifacts = match store.artifact_metadata(&session, "session") {
                    Ok(artifacts) => artifacts,
                    Err(error) => {
                        let message = format!("Artifact loading failed: {error:#}\nHistory remains available. Inspect the state directory before retrying.");
                        let key = (session.id.clone(), message.clone());
                        if self.artifact_diagnostic_key.as_ref() != Some(&key) {
                            self.artifact_diagnostic_key = Some(key);
                            self.raise_diagnostic(message);
                        }
                        Vec::new()
                    }
                };
                self.artifact_generation = Some(artifact_generation);
            }
        } else {
            self.attention = None;
            self.events.clear();
            self.activity_rows.clear();
            self.activity_cards = crate::activity_cards::Projection::default();
            self.activity_generation = None;
            self.artifacts.clear();
            self.artifact_generation = None;
            self.artifact_body_key = None;
            self.artifact_body = None;
            self.diagnostic = None;
            self.usage_error = None;
            self.usage_line = "Usage: no session selected".into();
            self.details.close();
            self.details.report = None;
            self.workspace = None;
            self.workspace_error = None;
            self.workspace_session = None;
            self.blocker_card = false;
            self.context_detail = None;
        }
        self.refresh_revert(store);
        self.refresh_checkpoint(store);
        self.refresh_run_details(store);
        if (self.view == View::Edits
            || self
                .session()
                .is_some_and(|s| matches!(s.status.as_str(), "blocked" | "interrupted")))
            && self.edits.pending.is_none()
        {
            self.refresh_edits(store);
        }
        if self.session().is_none() {
            self.result_open = false;
            self.result_key = None;
        }
        if changed_session {
            self.artifact_index = 0;
        } else if let Some(name) = old_artifact {
            self.artifact_index = self
                .artifacts
                .iter()
                .position(|artifact| artifact.name == name)
                .unwrap_or(0);
        } else {
            self.artifact_index = 0;
        }
        if changed_session || loaded_session_changed || following_tail {
            self.event_index = self.events.len().saturating_sub(1);
        } else if let Some(id) = old_event {
            self.event_index = self
                .events
                .iter()
                .position(|event| event.id == id)
                .unwrap_or(self.events.len().saturating_sub(1));
        } else {
            self.event_index = self.events.len().saturating_sub(1);
        }
        if let Some((session_id, branch)) = self.rerun_output.take() {
            if self
                .session()
                .is_some_and(|s| s.id == session_id && s.active_branch == branch)
            {
                if let Some(id) = self.activity_cards.tools.keys().max() {
                    if let Some(index) = self.events.iter().position(|event| event.id == *id) {
                        self.event_index = index;
                        self.view = View::Conversation;
                        self.result_open = false;
                        self.follow_activity = false;
                        self.inspector_open = true;
                        self.focus = Focus::Detail;
                        self.detail_scroll = 0;
                    }
                }
            }
        }
        self.load_selected_artifact(store);
        let event_changed = old_event != self.event().map(|e| e.id);
        if (changed_session || event_changed)
            && self.view == View::Conversation
            && self.diagnostic.is_none()
            && self.approval.is_none()
            && self.details.page.is_none()
            && self.context_detail.is_none()
            && !self.help
            && !self.blocker_card
            && !self.result_open
        {
            self.detail_scroll = 0;
        }
        if let Some(mut browser) = self.artifact_browser.take() {
            if let Some(session) = self.session() {
                if browser.session == session.id && browser.branch == session.active_branch {
                    match browser.refresh(store, session) {
                        Ok(()) => self.artifact_browser = Some(browser),
                        Err(error) => self.notice = format!("Version browser closed: {error:#}"),
                    }
                } else {
                    self.notice = "Version browser closed because session/branch changed.".into();
                }
            }
        }
        if self
            .diff
            .as_ref()
            .is_some_and(|(id, _)| self.event().map(|event| event.id) != Some(*id))
        {
            self.diff = None;
        }
        if reading_scope_changed {
            self.restore_reading();
        } else if self.view == View::Artifacts
            && old_artifact_version != self.artifact().map(|a| a.event_id)
            && !self.reading_overlay()
        {
            self.detail_scroll = 0;
        }
        self.load_selected_artifact(store);
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
                    self.activity_filtering = false;
                    self.artifact_browser = None;
                    self.help = false;
                    self.searching = false;
                    self.shell.sessions = false;
                    self.focus = self.main_focus();
                    self.session_query.clear();
                    self.new_session = None;
                    if let Some(id) = runner.session_id.as_ref() {
                        if let Some(index) = self.sessions.iter().position(|s| &s.id == id) {
                            self.session_index = index;
                            if self.workspace_session.as_deref() != Some(id) {
                                self.workspace = None;
                                self.context_detail = None;
                            }
                        }
                        self.requested_session = Some(id.clone());
                    }
                    self.shell.request_epoch += 1;
                    self.shell.actions_focus = false;
                    self.checkpoint = checkpoint::State::default();
                    self.approval_hidden = false;
                    self.approval_return_scroll = self.detail_scroll;
                    self.shell.more = false;
                    self.approval = Some(question);
                    self.detail_scroll = 0;
                }
                Err(TryRecvError::Disconnected | TryRecvError::Empty) => {}
            }
        }
        let stopped = runner
            .cancel_retry
            .as_ref()
            .is_some_and(|signal| signal.load(std::sync::atomic::Ordering::SeqCst));
        let tool_rerun = runner.tool_rerun;
        let completion = runner.finished.try_recv();
        let failed = !matches!(&completion, Ok(Ok(())) | Err(TryRecvError::Empty));
        let project_failure =
            if let Ok(Err(tui_actions::RunnerError::Project(conflict))) = &completion {
                Some(conflict.clone())
            } else {
                None
            };
        let rejected_session = runner.session_id.clone();
        if !matches!(&completion, Err(TryRecvError::Empty)) {
            if tool_rerun && matches!(&completion, Ok(Ok(()))) {
                if let Some(session) = self
                    .session()
                    .filter(|s| runner.session_id.as_deref() == Some(s.id.as_str()))
                {
                    self.rerun_output = Some((session.id.clone(), session.active_branch.clone()));
                }
            }
            self.notice = match completion {
                Ok(result) => match result {
                Ok(()) if stopped => "Execution stopped. Applied edits remain; review recorded work before continuing.".into(),
                Ok(()) if tool_rerun => "Tool rerun recorded. Workflow remains stopped; inspect its output, then continue when ready.".into(),
                Ok(()) => "Workflow finished. Review the result and recorded evidence.".into(),
                Err(tui_actions::RunnerError::Project(error)) => format!("Not started: {error}. No work was queued."),
                Err(error) => format!("Runner error: {error}"),
                },
                Err(_) => "Worker disconnected; pending approval expired. Inspect history and use /recover before resuming.".into(),
            };
            self.runner = None;
            self.approval = None;
            self.approval_hidden = false;
            if let Some(conflict) = project_failure {
                self.project = Some(conflict);
                if !tool_rerun {
                    if let Some((scope, text)) = self
                        .compose
                        .last_instruction
                        .clone()
                        .filter(|(scope, _)| scope.0 == rejected_session)
                    {
                        if self.compose.scope == scope && self.composer.is_empty() {
                            self.composer = text.clone();
                        }
                        let draft = self.drafts.entry(scope).or_default();
                        if draft.is_empty() {
                            *draft = text;
                        }
                        self.notice
                            .push_str(" Instruction retained as a draft; nothing was sent.");
                    }
                }
            } else if failed {
                self.raise_diagnostic(self.notice.clone());
            }
        }
    }

    fn submit(&mut self, root: &Path, store: &Store) {
        let original = self.composer.clone();
        let instruction = self.composer.trim().to_string();
        if instruction.is_empty() {
            return;
        }
        if let Some(text) = original.strip_prefix("/ask ") {
            self.start_manager(root, store, text.to_owned(), false);
            return;
        }
        if instruction == "/ask" {
            self.notice="Use /ask QUESTION for read-only manager inference; /manager-context previews shared context.".into();
            return;
        }
        if instruction == "/cancel-manager" {
            self.cancel_manager();
            self.composer.clear();
            return;
        }
        if instruction == "/manager" {
            self.view = View::Conversation;
            self.activity_query = "manager.".into();
            self.filter_activity();
            self.commit_activity_filter();
            self.composer.clear();
            self.focus = Focus::Events;
            self.follow_activity = false;
            return;
        }
        if instruction == "/manager-context" {
            if let Some(session) = self.session() {
                let artifacts = Vec::new();
                match crate::manager::capture(root, store, &session.id, "preview", &artifacts) {
                    Ok(snapshot) => {
                        self.context_detail = Some(format!(
                            "Manager context preview · no inference\n{}",
                            serde_json::to_string_pretty(&snapshot).unwrap_or_default()
                        ));
                        self.detail_scroll = 0;
                    }
                    Err(error) => {
                        self.raise_diagnostic(format!("Manager context unavailable: {error:#}"))
                    }
                }
            }
            self.composer.clear();
            return;
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
            self.open_help();
            self.composer.clear();
            return;
        }
        if instruction == "/dump" {
            self.open_conversation_dump(store);
            self.composer.clear();
            return;
        }
        if instruction == "/usage" {
            self.open_run_details(details::Page::Usage, store);
            self.composer.clear();
            return;
        }
        if instruction == "/context" {
            self.context_detail = Some(
                self.workspace
                    .as_ref()
                    .map(|w| {
                        format!(
                            "{}\n\nLatest recorded step states (loops may revisit):\n{}",
                            w.context(),
                            w.step_summary()
                        )
                    })
                    .unwrap_or_else(|| {
                        self.workspace_error
                            .clone()
                            .unwrap_or_else(|| "No session selected".into())
                    }),
            );
            self.composer.clear();
            self.detail_scroll = 0;
            return;
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
            } else {
                self.notice = "No session selected.".into();
            }
            self.composer.clear();
            return;
        }
        if instruction.starts_with('/') && instruction != "/resume" {
            self.notice = format!(
                "Unknown command: {}",
                instruction.lines().next().unwrap_or("")
            );
            return;
        }
        if instruction != "/resume" && self.manager_enabled {
            self.start_manager(root, store, original, true);
            return;
        }
        let input = if instruction == "/resume" {
            String::new()
        } else {
            instruction
        };
        self.resume_instruction(root, store, input);
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
fn safe_display(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
fn inline_display(text: &str) -> String {
    safe_display(text).replace(['\n', '\t'], " ")
}

fn draw_scrolled(
    frame: &mut Frame,
    app: &App,
    paragraph: Paragraph<'_>,
    area: ratatui::layout::Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let max = paragraph
        .line_count(area.width.saturating_sub(2))
        .saturating_sub(area.height as usize)
        .min(u16::MAX as usize) as u16;
    let offset = app.detail_scroll.min(max);
    app.rendered_scroll.set(Some((app.detail_scroll, offset)));
    frame.render_widget(paragraph.scroll((offset, 0)), area);
}

pub fn run(root: &Path, initial_session: Option<&str>) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("the TUI requires an interactive terminal (TTY)");
    }
    let root: PathBuf = root.canonicalize()?;
    let store = Store::open(&root)?;
    if let Some(id) = initial_session {
        store
            .session(id)
            .with_context(|| format!("unknown session {id}"))?;
    }
    let mut app = App::new(initial_session);
    app.shell.project = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
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
            let size = terminal.size()?;
            app.viewport = (size.width, size.height);
            app.tick();
            if captured_mouse != app.shell.mouse {
                shell::set_mouse(app.shell.mouse)?;
                captured_mouse = app.shell.mouse;
            }
            if let Some(job) = app.pending_artifact_review.take() {
                crossterm::execute!(
                    io::stdout(),
                    event::DisableMouseCapture,
                    event::DisableBracketedPaste
                )?;
                let edited = exports::with_terminal_suspended(|| job.edit())?;
                crossterm::execute!(io::stdout(), event::EnableBracketedPaste)?;
                shell::set_mouse(app.shell.mouse)?;
                captured_mouse = app.shell.mouse;
                terminal.clear()?;
                app.finish_artifact_review(&store, job, edited);
            }
            if let Some(snapshot) = app.pending_editor.take() {
                crossterm::execute!(
                    io::stdout(),
                    event::DisableMouseCapture,
                    event::DisableBracketedPaste
                )?;
                let result = exports::with_terminal_suspended(|| exports::open_editor(&snapshot))?;
                crossterm::execute!(io::stdout(), event::EnableBracketedPaste)?;
                shell::set_mouse(app.shell.mouse)?;
                captured_mouse = app.shell.mouse;
                terminal.clear()?;
                match result {
                    Ok(()) => {
                        app.notice = "Editor closed; snapshot changes were not imported.".into()
                    }
                    Err(error) => app.raise_diagnostic(format!("Editor unavailable: {error:#}")),
                }
            }
            if app.requested_session.is_some()
                || app.composer_session.as_deref() != app.session().map(|s| s.id.as_str())
                || last_refresh.elapsed() >= Duration::from_millis(600)
            {
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
    crossterm::execute!(
        io::stdout(),
        crossterm::cursor::SetCursorStyle::DefaultUserShape
    )?;
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
#[path = "tui_edits_tests.rs"]
mod edits_tests;

#[cfg(test)]
#[path = "tui_presentation_tests.rs"]
mod presentation_tests;

#[cfg(test)]
#[path = "tui_project_tests.rs"]
mod project_tests;

#[cfg(test)]
#[path = "tui_delivery_tests.rs"]
mod delivery_tests;
