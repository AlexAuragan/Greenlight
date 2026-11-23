//! Read-only run/usage presentation, scoped to the selected session and branch.
use super::*;
use crate::usage::{Call, Report, TokenCount, TokenTotals};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    GroupedEdits,
    Run,
    Technical,
    Provider,
    Usage,
    Requests,
    Request,
    Accounting,
    Conversation,
}
#[derive(Default)]
pub(super) struct Details {
    pub groups: Option<crate::edit_groups::Review>,
    pub group_index: usize,
    pub groups_by_step: bool,
    group_rendered: Option<(bool, usize)>,
    pub page: Option<Page>,
    pub report: Option<Report>,
    pub selected: Option<String>,
    pub configured_model: Option<String>,
    pub budget_config: Option<crate::model_budget::Config>,
    pub model_roles: Option<crate::config::ModelRoles>,
    pub provider: Option<crate::provider::ConnectionInfo>,
    pub text: String,
    pub dump: Option<crate::conversation_dump::Dump>,
    pub revision: u64,
    scope: Option<(String, String)>,
    back: Vec<(Page, u16)>,
    pub return_scroll: u16,
}
impl Details {
    pub fn close(&mut self) {
        self.page = None;
        self.groups = None;
        self.group_rendered = None;
        self.dump = None;
        self.back.clear();
        self.scope = None;
    }
    pub fn selected_call(&self) -> Option<&Call> {
        self.report
            .as_ref()?
            .calls
            .iter()
            .find(|c| Some(&c.id) == self.selected.as_ref())
    }
    pub fn actions(&self) -> Vec<shell::Action> {
        use shell::Action::*;
        match self.page {
            Some(Page::GroupedEdits) => vec![
                PreviousGroup,
                NextGroup,
                if self.groups_by_step {
                    GroupAttempts
                } else {
                    GroupSteps
                },
                DetailsBack,
            ],
            Some(Page::Conversation) => vec![Close, RefreshDump, SaveDump, CopyDump],
            Some(Page::Run) => vec![DetailsBack, Provider, Usage, Technical, ExportVerification],
            Some(Page::Technical) => vec![DetailsBack, Usage, Copy, ExportVerification],
            Some(Page::Usage) => {
                let mut actions = vec![DetailsBack];
                if self.report.as_ref().is_some_and(|r| !r.calls.is_empty()) {
                    actions.push(Requests);
                }
                actions.push(UsageAccounting);
                actions.push(Provider);
                actions
            }
            Some(Page::Requests) => {
                let mut actions = vec![DetailsBack];
                if self.selected_call().is_some() {
                    actions.push(OpenRequest);
                }
                actions
            }
            _ => vec![DetailsBack, Copy],
        }
    }
}
fn total(count: &TokenTotals, requests: u64) -> String {
    if count.unknown_requests == requests {
        return format!("Unknown · {requests} request(s)");
    }
    let known = count.reported.saturating_add(count.estimated);
    let source = if count.estimated_requests == 0 {
        "provider-reported"
    } else if requests.saturating_sub(count.estimated_requests + count.unknown_requests) == 0 {
        "estimated"
    } else {
        "mixed reported/estimated"
    };
    if count.unknown_requests > 0 {
        format!(
            "{known} known · {source}; {} request(s) unknown",
            count.unknown_requests
        )
    } else {
        format!("{known} · {source}")
    }
}
fn count(count: &TokenCount) -> String {
    count
        .value
        .map(|n| format!("{n} · {}", count.source))
        .unwrap_or_else(|| "Unknown".into())
}
fn usage_text(report: &Report) -> String {
    let t = &report.totals;
    if t.requests == 0 {
        return "Usage · this session\n\nNo recorded requests.\n\nHistorical calls are not backfilled. This does not establish that the session was free.\n\nScope: all recorded dispatches, all branches.".into();
    }
    let mut text = format!("Usage · this session\nAll branches · {} recorded dispatch(es), including failures/retries\n\nInput tokens: {}\nCached input: {}\nOutput tokens: {}\nCached input is a subset of input, not added again.\n\n", t.requests, total(&t.input, t.requests), total(&t.cached_input, t.requests), total(&t.output, t.requests));
    let label = if t.incomplete_cost_requests > 0 {
        "Partial estimate"
    } else {
        "Estimated cost"
    };
    if t.estimated_cost_by_currency.is_empty() {
        text.push_str(
            "Estimated cost: unavailable\nMissing usage/rates or unsupported billing rules.\n",
        );
    } else {
        for (currency, cost) in &t.estimated_cost_by_currency {
            text.push_str(&format!("{label}: {currency} {cost:.8}\n"));
        }
    }
    text.push_str(&format!(
        "Coverage: {} of {} requests have a usable cost estimate; {} uncovered.\n",
        t.requests.saturating_sub(t.incomplete_cost_requests),
        t.requests,
        t.incomplete_cost_requests
    ));
    if t.stale_pricing_requests > 0 {
        text.push_str(&format!(
            "Attention: {} request(s) used stale pricing snapshots.\n",
            t.stale_pricing_requests
        ));
    }
    let uncertain = report
        .calls
        .iter()
        .filter(|c| c.status != "response-received")
        .count();
    if uncertain > 0 {
        text.push_str(&format!("{uncertain} request(s) lack a successful complete response; billing may be uncertain.\n"));
    }
    text.push_str("\nHistorical pricing snapshots, not live prices or an invoice.\nUnknown usage is not zero. Historical calls are not backfilled.\nRequest details retain count sources and captured pricing.");
    text
}
fn request_text(call: &Call) -> String {
    let mut text = format!("Request details · recorded dispatch\n\nRequested model: {}\nReturned model: {}\nStatus: {}\nPurpose: {}\nStep: {}\nStarted: {}\n\nInput: {}\nCached input (subset): {}\nOutput: {}\nCache creation (subset): {}\nReasoning tokens: {}\n\n",call.model,call.response_model.as_deref().unwrap_or("Not recorded"),call.status,call.purpose,call.step,call.started_at_utc,count(&call.counts.input),count(&call.counts.cached_input),count(&call.counts.output),count(&call.counts.cache_creation),call.counts.reasoning.map(|n|n.to_string()).unwrap_or_else(||"Not recorded".into()));
    match (call.estimated_cost, call.metadata.pricing.as_ref()) {
        (Some(cost), Some(p)) => {
            text.push_str(&format!("Estimated cost: {} {cost:.8}\n", p.currency))
        }
        _ => text.push_str("Estimated cost: unavailable; unknown is not zero.\n"),
    }
    if let Some(p) = &call.metadata.pricing {
        text.push_str("\nCaptured pricing metadata (per million tokens)\n");
        // Only captured usage/pricing fields, never credentials or provider configuration.
        text.push_str(&serde_json::to_string_pretty(p).unwrap_or_else(|_| "Unavailable".into()));
        text.push('\n');
    } else {
        text.push_str("Pricing snapshot: Not recorded\n");
    }
    if let Some(reservation) = &call.reservation {
        text.push_str(&format!("\nDispatch reservation (estimates, not additional billed usage):\nInput {} · max output {} · estimated USD {:?}\n",reservation.input_tokens,reservation.output_tokens,reservation.estimated_cost_usd));
    }
    if !call.counts.notes.is_empty() {
        text.push_str(&format!(
            "\nAccounting notes\n{}\n",
            call.counts.notes.join("\n")
        ));
    }
    text.push_str(&format!("\nRecorded identity\nRequest: {}\nSession: {}\nBranch: {}\nAttempt: {}\nDispatch event: {}\nResponse ID: {}\n\nSnapshot only; historical charges are not repriced. Failed/interrupted requests may still incur charges.",call.id,call.session,call.branch,call.attempt.as_deref().unwrap_or("Not recorded"),call.created_event.map(|id|format!("#{id}")).unwrap_or_else(||"Not recorded".into()),call.response_id.as_deref().unwrap_or("Not recorded")));
    text
}
impl App {
    pub(super) fn open_grouped_review(&mut self, store: &Store) {
        let result = self
            .session()
            .context("No selected session")
            .and_then(|s| store.grouped_edits(s));
        match result {
            Ok(review) => {
                self.open_run_details(Page::GroupedEdits, store);
                if self.details.page != Some(Page::GroupedEdits)
                    || self.session().is_none_or(|s| {
                        s.id != review.session.id || s.active_branch != review.session.active_branch
                    })
                {
                    self.raise_diagnostic(
                        "Grouped snapshot scope changed; reopen the current branch.".into(),
                    );
                    return;
                }
                self.details.group_rendered = None;
                self.details.groups = Some(review);
                self.details.group_index = 0;
                self.details.groups_by_step = false;
                self.refresh_run_details(store);
                self.notice="Read-only grouped snapshot; [ / ] select groups, t toggles step/attempt. Reopen to refresh.".into();
            }
            Err(error) => self.raise_diagnostic(format!("Grouped review unavailable: {error:#}")),
        }
    }
    pub(super) fn open_conversation_dump(&mut self, store: &Store) {
        self.open_run_details(Page::Conversation, store);
        self.reload_conversation_dump(store);
    }
    pub(super) fn reload_conversation_dump(&mut self, store: &Store) {
        if self.details.page != Some(Page::Conversation) {
            return;
        }
        let Some(session) = self.session().cloned() else {
            return;
        };
        match crate::conversation_dump::Dump::load(
            store,
            &session.id,
            Some(&session.active_branch),
            false,
        ) {
            Ok(mut dump) => {
                if let Some(live) = self
                    .runner
                    .as_ref()
                    .filter(|r| r.session_id.as_deref() == Some(session.id.as_str()))
                    .and_then(|r| r.live.as_ref())
                    .and_then(|l| l.lock().ok())
                {
                    dump.live_preview = Some(format!(
                        "{} · {}\nBuffer snapshot; not linked to a specific durable call. {}\n{}",
                        live.model,
                        live.state,
                        if live.truncated {
                            "Earlier stream text was truncated."
                        } else {
                            ""
                        },
                        live.text
                    ));
                }
                self.details.dump = Some(dump);
                self.detail_scroll = 0;
                self.refresh_run_details(store);
            }
            Err(e) => {
                self.details.dump = None;
                self.details.text = format!("Conversation dump unavailable: {e:#}");
                self.notice = self.details.text.clone();
            }
        }
    }
    pub(super) fn save_conversation_dump(&mut self, store: &Store) {
        let result = (|| -> Result<std::path::PathBuf> {
            let dump = self
                .details
                .dump
                .as_ref()
                .context("No conversation snapshot loaded")?;
            let directory = store
                .directory
                .parent()
                .context("Missing project state directory")?
                .join("exports");
            let parent = crate::anchored_fs::Dir::open(
                directory
                    .parent()
                    .context("export directory lacks parent")?,
            )?;
            let anchored = parent.create_dirs(Path::new("exports"))?;
            let name = format!("conversation-{}.md", uuid::Uuid::new_v4());
            let path = directory.join(&name);
            anchored
                .target(Path::new(&name))?
                .publish_new(dump.markdown()?.as_bytes())?;
            Ok(path)
        })();
        match result {
            Ok(path) => self.notice = format!("Saved full conversation dump: {}", path.display()),
            Err(e) => self.notice = format!("Cannot save conversation dump: {e:#}"),
        }
    }
    pub(super) fn open_run_details(&mut self, page: Page, store: &Store) {
        self.remember_reading();
        if self.details.page.is_none() {
            if let Err(error) = self.refresh(store) {
                self.raise_diagnostic(format!("Run details unavailable: {error:#}"));
                return;
            }
        }
        let Some(s) = self.session().cloned() else {
            self.notice = "Choose a session first.".into();
            return;
        };
        let scope = (s.id.clone(), s.active_branch.clone());
        if self.details.page.is_none() || self.details.scope.as_ref() != Some(&scope) {
            self.details.close();
            self.details.return_scroll = self.detail_scroll;
            self.details.selected = None;
            if !self
                .details
                .report
                .as_ref()
                .is_some_and(|r| r.session == scope.0 && r.branch == scope.1)
            {
                self.details.report = store.usage_report(&s, crate::usage::Scope::Session).ok();
            }
            self.details.scope = Some(scope);
        } else if let Some(previous) = self.details.page {
            if previous != page {
                self.details.back.push((previous, self.detail_scroll));
            }
        }
        self.details.page = Some(page);
        self.detail_scroll = 0;
        self.shell.actions_focus = false;
        self.refresh_run_details(store);
    }
    pub(super) fn back_run_details(&mut self, store: &Store) {
        if let Some((page, scroll)) = self.details.back.pop() {
            self.details.page = Some(page);
            self.detail_scroll = scroll;
            self.refresh_run_details(store);
        } else {
            self.details.close();
            self.detail_scroll = self.details.return_scroll;
            self.restore_reading();
        }
        self.shell.actions_focus = false;
    }
    pub(super) fn move_usage_request(&mut self, delta: isize, store: &Store) {
        if let Some(report) = &self.details.report {
            let index = report
                .calls
                .iter()
                .position(|c| Some(&c.id) == self.details.selected.as_ref())
                .unwrap_or(0);
            let index = move_index(index, delta, report.calls.len());
            self.details.selected = report.calls.get(index).map(|c| c.id.clone());
            self.detail_scroll = index.saturating_sub(1).min(u16::MAX as usize) as u16;
        }
        self.refresh_run_details(store);
    }
    pub(super) fn refresh_run_details(&mut self, store: &Store) {
        let Some(page) = self.details.page else {
            return;
        };
        let Some(session) = self.session().cloned() else {
            self.details.close();
            return;
        };
        if self.details.scope.as_ref() != Some(&(session.id.clone(), session.active_branch.clone()))
        {
            self.details.close();
            return;
        }
        if page == Page::GroupedEdits {
            let key = (self.details.groups_by_step, self.details.group_index);
            if self.details.group_rendered != Some(key) {
                self.details.text = match &self.details.groups {
                    Some(review) => review
                        .text(key.0, key.1)
                        .unwrap_or_else(|error| format!("Grouped review unavailable: {error:#}")),
                    None => "Grouped review not loaded; reopen explicitly.".into(),
                };
                self.details.group_rendered = Some(key);
                self.details.revision = self.details.revision.wrapping_add(1);
            }
            return;
        }
        self.details.group_rendered = None;
        if self.details.selected.is_none() {
            self.details.selected = self
                .details
                .report
                .as_ref()
                .and_then(|r| r.calls.first())
                .map(|c| c.id.clone());
        }
        let workspace = self.workspace.as_ref();
        let ownership = match store.execution_state(&session.id) {
            Ok(crate::session_execution::ExecutionState::Live) => {
                if self
                    .runner
                    .as_ref()
                    .is_some_and(|r| r.session_id.as_deref() == Some(session.id.as_str()))
                {
                    "This TUI owns an active worker."
                } else {
                    "Another process owns execution; this viewer is observation-only."
                }
            }
            Ok(crate::session_execution::ExecutionState::Interrupted) => {
                "Interrupted ownership; recover owner before resuming. Recovery never resumes work."
            }
            Ok(crate::session_execution::ExecutionState::Idle) => {
                "No active execution owner recorded."
            }
            Err(_) => {
                "Execution ownership unavailable; inspect the recorded state before continuing."
            }
        };
        let mut text = match page {
            Page::GroupedEdits => unreachable!("grouped snapshots render above"),
            Page::Conversation => self.details.dump.as_ref().map(|d|format!("Conversation snapshot · previews limited to 800 characters per body\n[R] Refresh · [w] Save full dump · [C] Copy full dump\n\n{}",d.preview())).unwrap_or_else(||"Conversation dump not loaded; R refreshes the snapshot.".into()),
            Page::Run => {
                let mut text = format!("Run details · this session / active branch\n\nGoal\n{}\n\nWorkflow and current state\n{} · {} · {}\n", session.goal, session.workflow, session.status, session.current_step);
                if let Some(w) = workspace {
                    text.push_str("\nRoadmap · latest states, not a success percentage\n");
                    text.push_str(&w.steps.iter().map(|step|format!("{} {} ({:?})",step.state.marker(),step.id,step.state)).collect::<Vec<_>>().join(" → "));
                    text.push_str("\nLoops may revisit steps.\n");
                } else { text.push_str("\nRoadmap unavailable; technical details contain the load error.\n"); }
                text.push_str(&format!("\nExecution and evidence\n{ownership}"));
                text
            }
            Page::Provider => {
                if let Some(worker) = self.runner.as_ref().filter(|worker| worker.session_id.as_deref() == Some(session.id.as_str())) {
                    let live = worker.live.as_ref().map(|live| live.lock().unwrap());
                    match live.as_ref().and_then(|live| live.connection.as_ref()) {
                        Some(info) => format!("Provider / API key · active workflow worker\n\n{}\n\nLatest worker request model: {}\n\nThis is the worker's captured credential, including while a retry waits.", info.text(), live.as_ref().map(|live|live.model.as_str()).filter(|model|!model.is_empty()).unwrap_or("No request started yet")),
                        None => "Provider / API key\n\nWorker snapshot not available yet; waiting for initialization. No viewer credential has been substituted.".into(),
                    }
                } else {
                    self.details.provider.as_ref().map(|info| format!("Provider / API key · current viewer process\n\n{}\n\n{ownership}\nThis is not a historical request's key or another process's worker credential.", info.text()))
                        .unwrap_or_else(|| "Provider configuration unavailable in this viewer.".into())
                }
            }
            Page::Technical => {
                let mut text=format!("Technical details · selected session / active branch\n\nSession: {}\nBranch: {}\nState: {}\nStep: {}\nExecution: {ownership}\nState directory: {}\n\nConfigured model (current project configuration): {}\n",session.id,session.active_branch,session.status,session.current_step,store.directory.display(),self.details.configured_model.as_deref().unwrap_or("Unavailable"));
                let history = store.history(&session);
                let call = self.details.report.as_ref().and_then(|r| r.calls.iter().rev().find(|c| c.created_event.is_some_and(|id| history.as_ref().is_ok_and(|h|h.iter().any(|e|e.0==id))) && c.step == session.current_step && c.purpose != "manager" && workspace.is_some_and(|w|w.attempt.as_ref().is_some_and(|a|Some(&a.id)==c.attempt.as_ref()))));
                if let Some(c) = call { text.push_str(&format!("Latest request in recorded attempt: {}\nRequested model: {}\nReturned model: {}\n",c.id,c.model,c.response_model.as_deref().unwrap_or("Not recorded"))); }
                else { text.push_str("Requested model for recorded attempt: Not recorded\nReturned model for recorded attempt: Not recorded\n"); }
                if let Some(w) = workspace {
                    text.push_str(&format!("\nRecorded context\n{}\nRoadmap · latest states (loops may revisit)\n{}\n", w.context(), w.stepper(&session.current_step, usize::MAX)));
                    if let Some(a) = &w.attempt {
                        match history.as_ref().ok().and_then(|h|h.iter().find(|e|e.0==a.definition_event && e.2=="step.definition")) {
                            Some((id,_,_,raw))=>text.push_str(&format!("\nExact recorded definition · event #{id}\n{raw}\n")),
                            None => text.push_str("Exact recorded definition: Unavailable\n"),
                        }
                    } else { text.push_str("Attempt ID and exact definition: Not recorded\n"); }
                }
                if let Some(error) = &self.workspace_error { text.push_str(&format!("\n{error}\n")); }
                text
            }
            Page::Usage => self.details.report.as_ref().map(usage_text).unwrap_or_else(||"Usage unavailable\n\nAccounting could not be read. Missing data is not zero usage or cost.".into()),
            Page::Accounting => self.details.report.as_ref().map(Report::display).unwrap_or_else(||"Usage accounting unavailable".into()),
            Page::Request => self.details.selected_call().map(request_text).unwrap_or_else(||"Selected request unavailable\n\nThe recorded request could not be found. No other request has been substituted.".into()),
            Page::Requests => {
                let mut text = "Recorded requests · all session branches\n\n".to_string();
                if let Some(r) = &self.details.report {
                    for c in &r.calls { text.push_str(&format!("{} · {} · {} · {} · {}\n",c.id,c.step,c.purpose,c.model,c.status)); }
                }
                text
            },
        };
        if matches!(page, Page::Provider | Page::Request) {
            let mut scope = session.clone();
            if page == Page::Request {
                if let Some(call) = self.details.selected_call() {
                    scope.active_branch = call.branch.clone();
                }
            }
            if let Ok(history) = store.history(&scope) {
                let evidence = history
                    .iter()
                    .rev()
                    .filter(|event| event.2 == "provider.failure")
                    .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event.3).ok())
                    .find(|value| {
                        page == Page::Provider
                            || self.details.selected.as_deref() == value["call_id"].as_str()
                    });
                if let Some(value) = evidence {
                    text.push_str(&format!(
                        "\n\nRecorded API error · request {}\n",
                        value["call_id"].as_str().unwrap_or("unknown")
                    ));
                    if let Ok(failure) = serde_json::from_value::<crate::provider_failure::Failure>(
                        value["failure"].clone(),
                    ) {
                        text.push_str(&format!("{failure}\n"));
                        if let Some(diagnostic) = failure.diagnostic {
                            text.push_str(&diagnostic.text());
                        } else {
                            text.push_str("No original provider response text was recorded; older discarded bodies cannot be backfilled.");
                        }
                    }
                }
            }
        }
        if matches!(page, Page::Usage | Page::Technical) {
            if let Some(config) = &self.details.budget_config {
                text.push_str(&format!(
                    "\n\n{}\n",
                    store
                        .model_budget_status(&session, config)
                        .unwrap_or_else(|e| format!("Model budgets unavailable: {e:#}"))
                ));
            }
            if page == Page::Technical {
                if let Some(roles) = &self.details.model_roles {
                    text.push_str(&format!(
                        "\nConfigured model role defaults (same provider/account):\n{}",
                        serde_yaml::to_string(roles).unwrap_or_default()
                    ));
                }
            }
        }
        if self.details.text != text {
            self.details.text = text;
            self.details.revision = self.details.revision.wrapping_add(1);
        }
    }
}
pub(super) fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(page) = app.details.page else {
        return;
    };
    if page == Page::Requests {
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        frame.render_widget(
            Paragraph::new(
                "Recorded requests · all branches\nSelect a dispatch · Enter opens its details",
            )
            .style(Style::default().fg(theme::MUTED)),
            rows[0],
        );
        let calls = app
            .details
            .report
            .as_ref()
            .map(|r| r.calls.as_slice())
            .unwrap_or(&[]);
        let selected = calls
            .iter()
            .position(|c| Some(&c.id) == app.details.selected.as_ref());
        let items = calls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                ListItem::new(super::view::fit(
                    &format!("{} · {} · {} · {}", i + 1, c.step, c.purpose, c.model),
                    rows[1].width.saturating_sub(2) as usize,
                ))
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default().with_selected(selected);
        frame.render_stateful_widget(
            List::new(items).highlight_symbol("› ").highlight_style(
                Style::default()
                    .fg(theme::FOCUS)
                    .bg(theme::PANEL)
                    .add_modifier(Modifier::BOLD),
            ),
            rows[1],
            &mut state,
        );
        let offset = state.offset();
        for (i, _) in calls
            .iter()
            .enumerate()
            .skip(offset)
            .take(rows[1].height as usize)
        {
            app.shell.hit(
                ratatui::layout::Rect::new(
                    rows[1].x,
                    rows[1].y + (i - offset) as u16,
                    rows[1].width,
                    1,
                ),
                shell::Action::UsageRequest(i),
            );
        }
    } else if matches!(page, Page::Run | Page::Usage)
        && app.details.text.len() < 8192
        && app.details.text.lines().count() < 64
    {
        let mut text = crate::markdown::raw(&app.details.text);
        let mut goal = false;
        for (i, line) in text.lines.iter_mut().enumerate() {
            let content = line.to_string();
            if content == "Goal" {
                goal = true;
            } else if content.is_empty() {
                goal = false;
            }
            let heading = i == 0
                || matches!(
                    content.as_str(),
                    "Goal"
                        | "Workflow and current state"
                        | "Execution and evidence"
                        | "Selected scope"
                )
                || content.starts_with("Roadmap ·");
            line.style = if heading {
                Style::default().add_modifier(Modifier::BOLD)
            } else if goal {
                Style::default().fg(theme::USER)
            } else if content.starts_with("Partial estimate:")
                || content.starts_with("Estimated cost: unavailable")
                || content.starts_with("Attention:")
            {
                Style::default()
                    .fg(theme::ATTENTION)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };
        }
        super::draw_scrolled(
            frame,
            app,
            Paragraph::new(text).wrap(Wrap { trim: false }),
            area,
        );
    } else {
        let (title, source) = app
            .details
            .text
            .split_once('\n')
            .unwrap_or((&app.details.text, ""));
        let draw = if page == Page::GroupedEdits {
            reader::draw_diff_document
        } else {
            reader::draw_document
        };
        draw(
            frame,
            app,
            reader::Document {
                tag: format!("run-details:{page:?}:{}", app.details.revision),
                source,
                markdown: false,
                headers: vec![Line::styled(
                    title.to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                )],
            },
            Block::default(),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Counts, ModelMetadata, Pricing};
    fn fixture() -> (tempfile::TempDir, Store, Session, App) {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store
            .create(
                "fix-bug",
                "Investigate the café startup delay",
                "investigate",
            )
            .unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        (root, store, session, app)
    }
    pub(super) fn call(store: &Store, s: &Session, model: &str, currency: Option<&str>) -> Call {
        let pricing = currency.map(|currency| Pricing {
            input_per_million: Some(1.0),
            cached_input_per_million: Some(0.25),
            output_per_million: Some(2.0),
            currency: currency.into(),
            source: "fixture catalog".into(),
            effective_date: Some("2026-11-01".into()),
            retrieved_at: Some("2026-11-17".into()),
            cache_creation: None,
            tiers: vec![],
        });
        let mut c = Call::new(
            s,
            &s.current_step,
            "agent",
            model,
            "fixture",
            ModelMetadata {
                pricing,
                ..Default::default()
            },
            &[],
        );
        store.begin_model_call(s, &mut c).unwrap();
        c.status = "response-received".into();
        let reported = |n| TokenCount {
            value: Some(n),
            source: "provider".into(),
        };
        c.counts = Counts {
            input: reported(1000),
            cached_input: reported(250),
            cache_creation: reported(0),
            output: reported(50),
            reasoning: Some(5),
            raw: None,
            notes: vec![],
        };
        c.response_model = Some(model.into());
        c.estimated_cost = currency.map(|_| 0.001);
        store.finish_model_call(s, &c).unwrap();
        c
    }
    #[test]
    fn session_usage_preserves_unknowns_sources_partial_coverage_and_currencies() {
        let (root, store, mut session, mut app) = fixture();
        let usd = call(&store, &session, "priced", Some("USD"));
        let mut interrupted = call(&store, &session, "unpriced", None);
        interrupted.status = "started-or-interrupted".into();
        interrupted.response_model = None;
        interrupted.counts.input = TokenCount {
            value: Some(20),
            source: "tiktoken/cl100k_base".into(),
        };
        interrupted.counts.cached_input = TokenCount {
            value: None,
            source: "unknown".into(),
        };
        interrupted.counts.output = TokenCount {
            value: None,
            source: "unknown".into(),
        };
        store.finish_model_call(&session, &interrupted).unwrap();
        let event = store.event(&session, None, "fixture.fork", "").unwrap();
        store.fork(&mut session, event, "investigate").unwrap();
        call(&store, &session, "euro", Some("EUR"));
        app.refresh(&store).unwrap();
        app.open_run_details(Page::Usage, &store);
        let text = &app.details.text;
        assert!(text.contains("3 recorded dispatch"));
        assert!(text.contains("2020 · mixed"));
        assert!(text.contains("1 request(s) unknown"));
        assert!(text.contains("Partial estimate: USD"));
        assert!(text.contains("Partial estimate: EUR"));
        assert!(text.contains("2 of 3 requests"));
        assert!(text.contains("billing may be uncertain"));
        assert_eq!(app.details.report.as_ref().unwrap().totals.requests, 3);
        app.details.selected = Some(usd.id.clone());
        app.open_run_details(Page::Request, &store);
        assert!(app.details.text.contains(&usd.id));
        assert!(app.details.text.contains("fixture catalog"));
        assert!(app.details.text.contains("2026-11-01"));
        assert!(app.details.text.contains("Reasoning tokens: 5"));
        assert_eq!(app.export_snapshot().unwrap().text, app.details.text);
        assert!(!app.details.text.contains("api_key"));
        app.on_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.details.page, Some(Page::Usage));
    }
    #[test]
    fn request_identity_survives_refresh_and_missing_records_are_not_substituted() {
        let (root, store, session, mut app) = fixture();
        let first = call(&store, &session, "first", None);
        app.refresh(&store).unwrap();
        app.open_run_details(Page::Usage, &store);
        app.open_run_details(Page::Requests, &store);
        app.open_run_details(Page::Request, &store);
        app.detail_scroll = 4;
        call(&store, &session, "second", None);
        store
            .event(
                &session,
                None,
                "user.message",
                "New recorded activity while reading",
            )
            .unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.details.selected.as_deref(), Some(first.id.as_str()));
        assert_eq!(app.detail_scroll, 4);
        assert!(app.details.text.contains("Requested model: first"));
        store
            .db
            .execute("DELETE FROM model_calls WHERE id=?1", [&first.id])
            .unwrap();
        app.refresh(&store).unwrap();
        assert!(app.details.text.contains("Selected request unavailable"));
        assert!(!app.details.text.contains("Requested model: second"));
        app.on_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.details.page, Some(Page::Requests));
        assert!(!app.details.actions().contains(&shell::Action::OpenRequest));
        app.move_usage_request(1, &store);
        assert_eq!(app.details.selected_call().unwrap().model, "second");
    }
    #[test]
    fn run_context_uses_recorded_attempt_model_and_closes_when_scope_changes() {
        let (root, store, mut session, mut app) = fixture();
        let definition = "type: agent\nprompt: Original investigation\noutputs:\n  required: []\n";
        let attempt = store
            .begin_attempt(&session, "investigate", definition, "attempt-model")
            .unwrap();
        session.status = "blocked".into();
        store.save(&session).unwrap();
        let mut original = call(&store, &session, "requested-model", None);
        original.response_model = Some("returned-model".into());
        store.finish_model_call(&session, &original).unwrap();
        app.refresh(&store).unwrap();
        app.open_run_details(Page::Run, &store);
        assert!(app.details.text.contains("Investigate the café"));
        assert!(!app.details.text.contains(&session.id));
        app.open_run_details(Page::Technical, &store);
        assert!(app.details.text.contains(&attempt));
        assert!(app.details.text.contains("Original investigation"));
        assert!(app
            .details
            .text
            .contains("Requested model: requested-model"));
        assert!(app.details.text.contains("Returned model: returned-model"));
        assert!(app.details.text.contains("current project configuration"));
        let before = store.history(&session).unwrap().len();
        let calls = store
            .usage_report(&session, crate::usage::Scope::Session)
            .unwrap()
            .totals
            .requests;
        app.open_help();
        app.on_key(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        app.on_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.details.page, Some(Page::Technical));
        assert_eq!(store.history(&session).unwrap().len(), before);
        assert_eq!(
            store
                .usage_report(&session, crate::usage::Scope::Session)
                .unwrap()
                .totals
                .requests,
            calls
        );
        let parent_session = session.clone();
        let event = store.event(&session, None, "fixture.fork", "").unwrap();
        store.fork(&mut session, event, "investigate").unwrap();
        call(&store, &parent_session, "later-parent-request", None);
        app.refresh(&store).unwrap();
        assert!(app.details.page.is_none());
        app.open_run_details(Page::Technical, &store);
        assert!(app
            .details
            .text
            .contains("Requested model: requested-model"));
        assert!(!app
            .details
            .text
            .contains("Requested model: later-parent-request"));
    }
    #[test]
    fn empty_usage_legacy_context_and_back_actions_preserve_workspace_reading() {
        let (root, store, session, mut app) = fixture();
        store
            .event(
                &session,
                Some("investigate"),
                "tool.output",
                "A long saved record",
            )
            .unwrap();
        app.refresh(&store).unwrap();
        app.follow_activity = false;
        app.focus = Focus::Detail;
        app.inspector_open = true;
        app.detail_scroll = 7;
        app.open_run_details(Page::Run, &store);
        app.open_run_details(Page::Technical, &store);
        assert!(app
            .details
            .text
            .contains("Attempt ID and exact definition: Not recorded"));
        app.back_run_details(&store);
        app.open_run_details(Page::Usage, &store);
        assert!(app.details.text.contains("No recorded requests"));
        assert!(!app.details.text.contains("Estimated cost: USD 0"));
        assert!(!app.details.actions().contains(&shell::Action::Requests));
        app.back_run_details(&store);
        app.back_run_details(&store);
        assert!(app.details.page.is_none());
        assert_eq!(app.detail_scroll, 7);
        assert_eq!(app.focus, Focus::Detail);
        let mut finished = session.clone();
        finished.status = "completed".into();
        store.save(&finished).unwrap();
        app.refresh(&store).unwrap();
        app.detail_scroll = 3;
        app.open_run_details(Page::Run, &store);
        app.back_run_details(&store);
        assert_eq!(app.detail_scroll, 3);
        app.result_open = false;
        let other = store
            .create("fix-bug", "Another run", "investigate")
            .unwrap();
        app.open_run_details(Page::Run, &store);
        app.requested_session = Some(other.id);
        app.refresh(&store).unwrap();
        assert!(app.details.page.is_none());
        assert!(app.runner.is_none());
        assert!(app.approval.is_none());
        let _ = root;
    }
    #[test]
    fn request_navigation_and_mouse_targets_revalidate_after_refresh() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{backend::TestBackend, Terminal};
        let (root, store, session, mut app) = fixture();
        let first = call(&store, &session, "first-request", None);
        app.refresh(&store).unwrap();
        app.open_run_details(Page::Requests, &store);
        app.viewport = (80, 24);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| view::draw(f, &app)).unwrap();
        let y = terminal
            .backend()
            .buffer()
            .content
            .chunks(80)
            .position(|row| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .contains("first-request")
            })
            .unwrap() as u16;
        let click = |app: &mut App, kind| {
            app.on_event(
                Event::Mouse(MouseEvent {
                    kind,
                    column: 2,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                }),
                root.path(),
                &store,
            )
            .unwrap();
        };
        click(&mut app, MouseEventKind::Down(MouseButton::Left));
        let second = call(&store, &session, "second-request", None);
        app.refresh(&store).unwrap();
        terminal.draw(|f| view::draw(f, &app)).unwrap();
        click(&mut app, MouseEventKind::Up(MouseButton::Left));
        assert_eq!(app.details.page, Some(Page::Requests));
        click(&mut app, MouseEventKind::Down(MouseButton::Left));
        click(&mut app, MouseEventKind::Up(MouseButton::Left));
        assert_eq!(app.details.page, Some(Page::Request));
        assert_eq!(app.details.selected.as_deref(), Some(first.id.as_str()));
        app.back_run_details(&store);
        app.on_key(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.details.selected.as_deref(), Some(second.id.as_str()));
        app.on_key(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.details.selected.as_deref(), Some(first.id.as_str()));
        app.on_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        app.on_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(
            app.shell_actions()[app.shell.action_index],
            shell::Action::OpenRequest
        );
        app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.details.page, Some(Page::Request));
    }
    #[test]
    fn conversation_snapshot_is_read_only_refreshable_complete_on_export_and_scoped() {
        let (_root, store, session, mut app) = fixture();
        let raw = format!("{}FULL_RESPONSE_END", "visible response ".repeat(300));
        store
            .event(&session, Some("investigate"), "model.output", &raw)
            .unwrap();
        let before = store.history(&session).unwrap();
        app.open_conversation_dump(&store);
        assert_eq!(app.details.page, Some(Page::Conversation));
        assert!(!app.details.text.contains("FULL_RESPONSE_END"));
        assert!(app.export_snapshot().unwrap().text.contains(&raw));
        app.save_conversation_dump(&store);
        assert!(app.notice.starts_with("Saved full conversation dump:"));
        let exports = store.directory.parent().unwrap().join("exports");
        let path = std::fs::read_dir(exports)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(std::fs::read_to_string(path).unwrap().contains(&raw));
        assert_eq!(store.history(&session).unwrap(), before);
        assert_eq!(store.session(&session.id).unwrap().status, "ready");
        store
            .event(
                &session,
                Some("investigate"),
                "protocol.error",
                "unknown action shell",
            )
            .unwrap();
        assert!(!app.details.text.contains("unknown action shell"));
        app.reload_conversation_dump(&store);
        assert!(app.details.text.contains("unknown action shell"));
        let other = store
            .create("fix-bug", "different session", "investigate")
            .unwrap();
        app.requested_session = Some(other.id.clone());
        app.refresh(&store).unwrap();
        assert!(app.details.page.is_none());
        assert!(app.details.dump.is_none());
    }

    #[test]
    fn conversation_live_buffer_is_explicit_and_never_leaks_between_sessions() {
        let (_root, store, session, mut app) = fixture();
        let live =
            std::sync::Arc::new(std::sync::Mutex::new(crate::provider::stream::LiveOutput {
                model: "fixture".into(),
                text: "PARTIAL_VISIBLE_TEXT".into(),
                state: "Receiving model output…".into(),
                truncated: true,
                connection: None,
            }));
        app.runner = Some(Runner {
            tool_rerun: false,
            live: Some(live),
            cancel_retry: None,
            session_id: Some(session.id.clone()),
            prompts: std::sync::mpsc::channel().1,
            finished: std::sync::mpsc::channel().1,
            answers: std::sync::mpsc::channel().0,
        });
        app.open_conversation_dump(&store);
        let dump = app.details.dump.as_ref().unwrap();
        assert!(dump
            .live_preview
            .as_ref()
            .unwrap()
            .contains("PARTIAL_VISIBLE_TEXT"));
        assert!(dump.live_preview.as_ref().unwrap().contains("truncated"));
        assert!(dump
            .markdown()
            .unwrap()
            .contains("not validated or executed evidence"));
        assert!(
            crate::conversation_dump::Dump::load(&store, &session.id, None, false)
                .unwrap()
                .live_preview
                .is_none()
        );
        let other = store.create("fix-bug", "other", "investigate").unwrap();
        app.requested_session = Some(other.id);
        app.refresh(&store).unwrap();
        app.open_conversation_dump(&store);
        assert!(app.details.dump.as_ref().unwrap().live_preview.is_none());
        assert!(!app
            .export_snapshot()
            .unwrap()
            .text
            .contains("PARTIAL_VISIBLE_TEXT"));
    }
    #[test]
    fn provider_page_shows_worker_snapshot_without_persisting_or_exporting_full_keys() {
        let (root, store, session, mut app) = fixture();
        let name = format!("GREENLIGHT_PROVIDER_UI_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&name, "start-private-middle-first");
        let mut cfg = crate::config::load_config(root.path()).unwrap().provider;
        cfg.api_key_env = name.clone();
        cfg.base_url = "https://user:password@example.test/v1?token=url-secret".into();
        let provider = crate::provider::Provider::new(cfg.clone());
        let live = crate::provider::stream::Preview::default();
        live.lock().unwrap().connection = Some(provider.connection_info());
        // A new model turn preserves the worker's credential metadata.
        live.lock().unwrap().start("step-model");
        std::env::set_var(&name, "other-private-middle-final");
        let (_prompts, prompts) = std::sync::mpsc::channel();
        let (answers, _answers) = std::sync::mpsc::channel();
        let (_finished, finished) = std::sync::mpsc::channel();
        app.runner = Some(Runner {
            tool_rerun: false,
            live: Some(live),
            cancel_retry: None,
            session_id: Some(session.id.clone()),
            prompts,
            answers,
            finished,
        });
        app.details.provider = Some(crate::provider::ConnectionInfo::current(&cfg));
        let before = store.history(&session).unwrap();
        app.open_run_details(Page::Provider, &store);
        assert!(app.details.text.contains("active workflow worker"));
        assert!(app.details.text.contains("start…first"));
        assert!(app.details.text.contains("step-model"));
        assert!(!app.details.text.contains("other…final"));
        let exported = app.export_snapshot().unwrap().text;
        for secret in [
            "start-private-middle-first",
            "other-private-middle-final",
            "password",
            "url-secret",
        ] {
            assert!(!exported.contains(secret), "{secret}");
        }
        assert_eq!(store.history(&session).unwrap(), before);
        app.runner = None;
        app.details.provider = Some(crate::provider::ConnectionInfo::current(&cfg));
        app.refresh_run_details(&store);
        assert!(app.details.text.contains("current viewer process"));
        assert!(app.details.text.contains("other…final"));
        std::env::remove_var(name);
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn provider_details_show_copyable_recorded_error_text_and_support_id() {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("fix-bug", "goal", "investigate").unwrap();
        let mut failure = crate::provider_failure::Failure::http(
            429,
            &serde_json::json!({"detail":"Model busy"}),
            None,
        );
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-request-id", "support-id-123".parse().unwrap());
        failure.diagnostic = Some(crate::provider_diagnostic::Diagnostic::capture(
            r#"{"detail":"Model busy"}"#,
            &headers,
            "",
        ));
        store.event(&session, Some("investigate"), "provider.failure", &serde_json::json!({"call_id":"original-call","purpose":"coding","failure":failure}).to_string()).unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        app.open_run_details(Page::Provider, &store);
        assert!(app.details.text.contains("Model busy"));
        assert!(app.details.text.contains("support-id-123"));
        assert!(app.details.text.contains("original-call"));
        assert!(app.details.actions().contains(&shell::Action::Copy));
    }
}
