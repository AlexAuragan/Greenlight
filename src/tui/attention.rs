//! Evidence-scoped attention presentation. Does not change recovery/retry policy.
use super::*;
use crate::provider_failure::{Failure, Kind as ProviderKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Blocked,
    Provider,
    Owner,
    Writes,
    Restore,
    Recovered,
    Observing,
    Retry,
}
pub(super) struct State {
    pub kind: Kind,
    pub title: String,
    pub reason: String,
    pub next: &'static str,
    pub impact: &'static str,
    pub evidence: Option<i64>,
    pub resumable: bool,
}
impl State {
    pub(super) fn color(&self) -> Color {
        match self.kind {
            Kind::Provider | Kind::Writes | Kind::Restore => theme::ERROR,
            Kind::Recovered => theme::GOOD,
            Kind::Observing | Kind::Retry => theme::FOCUS,
            _ => theme::ATTENTION,
        }
    }
    pub(super) fn actions(&self) -> Vec<shell::Action> {
        use shell::Action::*;
        match self.kind {
            Kind::Owner => vec![RecoverOwner, InspectAttention, More],
            Kind::Writes => vec![WriteGuidance, RecordedActivity, More],
            Kind::Retry => vec![Stop, InspectAttention, More],
            Kind::Observing => vec![RecordedActivity, More],
            _ if self.resumable => vec![InspectAttention, GiveInstructions, More],
            _ => vec![InspectAttention, More],
        }
    }
}
pub(super) fn project(
    session: &Session,
    workspace: &crate::workspace_view::SessionWorkspace,
    history: &[(i64, String, String, String)],
    local: bool,
    calls: &[crate::usage::Call],
) -> Option<State> {
    use crate::workspace_view::PrimaryAction as Primary;
    let evidence = workspace.blocker.as_ref().map(|b| b.event_id);
    let reason = workspace
        .blocker
        .as_ref()
        .map(|b| b.reason.clone())
        .unwrap_or_else(|| "No blocking reason recorded. Inspect the available history.".into());
    let mut state = State {
        kind: Kind::Blocked,
        title: "Step needs attention".into(),
        reason,
        next: "Review the evidence before continuing.",
        impact: "Execution is stopped; recorded work remains available.",
        evidence,
        resumable: workspace.primary == Primary::Resume && !local,
    };
    match workspace.primary {
        Primary::RecoverOwner => {
            state.kind = Kind::Owner;
            state.title = "Previous execution needs owner recovery".into();
            state.reason="The execution record has no live owner lock. Recovery checks the lock again and refuses a live owner.".into();
            state.next = "Recover owner, then inspect unfinished writes before continuing.";
            state.impact =
                "Recovery expires unanswered approvals. Saved grants remain. No work is resumed.";
            return Some(state);
        }
        Primary::RecoverWrites => {
            state.kind = Kind::Writes;
            state.title = "An interrupted native write needs reconciliation".into();
            state.reason="The workspace has unresolved native-write records. Do not assume the intended file change was applied or reverted.".into();
            state.next = "Open recovery guidance for the supported CLI recover-writes flow.";
            state.impact =
                "Resume and restart remain unavailable until write recovery is resolved.";
            return Some(state);
        }
        Primary::Observe if !local => {
            state.kind = Kind::Observing;
            state.title = "Observing another process".into();
            state.reason="Another worker owns this session's execution. Activity and saved artifacts can be inspected here.".into();
            state.next = "Use the owning process for decisions and execution control.";
            state.impact = "This viewer cannot approve its tools, resume it or take over its lock.";
            return Some(state);
        }
        _ => {}
    }
    if let Some((id, _, kind, _)) = history.iter().rev().find(|e| {
        matches!(
            e.2.as_str(),
            "workspace.restore-failed" | "workspace.restore-completed"
        )
    }) {
        if kind == "workspace.restore-failed" {
            state.kind = Kind::Restore;
            state.evidence = Some(*id);
            state.title = "Workspace restoration failed".into();
            state.next =
                "Inspect the full failure and workspace before attempting any further changes.";
            state.impact =
                "Some files may already have changed. This is not an all-or-nothing restore.";
            return Some(state);
        }
    }
    let start = history
        .iter()
        .rev()
        .find(|e| e.1 == session.current_step && e.2 == "step.started")
        .map(|e| e.0)
        .unwrap_or(0);
    let current = history
        .iter()
        .filter(|e| e.0 > start && e.1 == session.current_step)
        .collect::<Vec<_>>();
    let provider = current.iter().rev().find_map(|e| {
        if e.2 != "provider.failure" {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(&e.3).ok()?;
        if value["purpose"].as_str()? == "manager" {
            return None;
        }
        let failure = serde_json::from_value::<Failure>(value["failure"].clone()).ok()?;
        Some((e.0, value["call_id"].as_str()?.to_owned(), failure))
    });
    let non_manager_call = |id: &str| {
        calls
            .iter()
            .find(|c| c.id == id)
            .is_none_or(|c| c.purpose != "manager")
    };
    let last_control = current.iter().rev().find(|e| {
        (e.2 != "provider.retry"
            || serde_json::from_str::<serde_json::Value>(&e.3)
                .ok()
                .is_some_and(|v| v["purpose"].as_str().is_some_and(|p| p != "manager")))
            && (e.2 != "model.call-started" || non_manager_call(&e.3))
            && matches!(
                e.2.as_str(),
                "provider.retry"
                    | "model.call-started"
                    | "provider.retry-cancelled"
                    | "provider.retry-stopped"
                    | "step.completed"
                    | "step.blocked"
                    | "step.error"
                    | "runner.error"
            )
    });
    if local && session.status == "running" {
        if let Some(event) = last_control.filter(|e| e.2 == "provider.retry") {
            let value = serde_json::from_str::<serde_json::Value>(&event.3).ok()?;
            if value["purpose"].as_str().is_some_and(|p| p != "manager")
                && provider
                    .as_ref()
                    .is_some_and(|(_, call, _)| Some(call.as_str()) == value["call_id"].as_str())
            {
                state.kind = Kind::Retry;
                state.title = "Waiting for the provider retry".into();
                state.evidence = Some(event.0);
                state.reason = provider.unwrap().2.to_string();
                state.next = "Wait for the bounded retry, or stop future requests.";
                state.impact =
                    "Stop cancels the retry wait. A request already in flight may finish.";
                return Some(state);
            }
        }
        return None;
    }
    if session.status != "blocked" {
        return None;
    }
    if let Some(blocker) = workspace
        .blocker
        .as_ref()
        .and_then(|b| history.iter().find(|e| e.0 == b.event_id))
    {
        if blocker.2 == "runner.recovered" {
            state.kind = Kind::Recovered;
            state.title = "Owner recovered; workflow remains stopped".into();
            state.next = "Review the evidence, then provide instructions when ready.";
            return Some(state);
        }
    }
    if let Some((id, call, failure)) = provider {
        // A later dispatch, successful output or recovery supersedes an earlier failure.
        let superseded = current.iter().any(|e| {
            e.0 > id
                && (e.2 != "model.call-started" || non_manager_call(&e.3))
                && matches!(
                    e.2.as_str(),
                    "model.call-started" | "model.output" | "step.completed" | "runner.recovered"
                )
        });
        if !superseded && !call.is_empty() {
            state.kind = Kind::Provider;
            state.evidence = Some(id);
            state.title = match failure.kind {
                ProviderKind::Credentials => "Provider credentials rejected",
                ProviderKind::Billing => "Provider credit or billing quota exhausted",
                ProviderKind::RateLimit => "Provider rate limit reached",
                ProviderKind::Permission => "Provider access denied",
                ProviderKind::Request => "Provider rejected the request",
                ProviderKind::Unavailable => "Provider service unavailable",
                ProviderKind::Timeout => "Provider request timed out",
                ProviderKind::Connection => "Provider connection failed",
                ProviderKind::Protocol => "Provider response could not be used",
                ProviderKind::Cancelled => "Provider retry cancelled",
                ProviderKind::Unknown => "Provider request failed",
            }
            .into();
            state.reason = failure.to_string();
            state.next = "Resolve the provider issue, then resume explicitly when ready.";
            state.impact = "The step is blocked. No automatic retry is currently active.";
        }
    }
    Some(state)
}
impl App {
    pub(super) fn attention_visible(&self) -> bool {
        self.view == View::Conversation
            && self.attention.as_ref().is_some_and(|state| {
                self.blocker_card
                    || state.kind == Kind::Retry && self.follow_activity && !self.inspector_open
            })
    }
    pub(super) fn clear_diagnostic(&mut self) {
        self.diagnostic = None;
        self.diagnostic_detail = false;
        self.diagnostic_attention = false;
        self.detail_scroll = 0;
    }
    pub(super) fn attention_history(&mut self) {
        self.clear_diagnostic();
        self.blocker_card = false;
        self.attention_return = false;
        self.result_open = false;
        self.view = View::Conversation;
        self.focus = Focus::Events;
        self.inspector_open = false;
        self.follow_activity = false;
    }
    pub(super) fn inspect_attention(&mut self) {
        let id = self.attention.as_ref().and_then(|s| s.evidence);
        if let Some(id) = id {
            if let Some(index) = self.events.iter().position(|e| e.id == id) {
                self.clear_diagnostic();
                self.event_index = index;
                self.blocker_card = false;
                self.follow_activity = false;
                self.view = View::Conversation;
                self.focus = Focus::Detail;
                self.inspector_open = true;
                self.attention_return = true;
                return;
            }
            if let Some(event) = self.activity_rows.iter().find(|e| e.id == id) {
                let detail = format!(
                    "Recorded evidence · event #{}\nStep: {} · {}\n\n{}",
                    event.id, event.step, event.kind, event.data
                );
                self.clear_diagnostic();
                self.context_detail = Some(detail);
                return;
            }
        }
        if self.diagnostic.is_some() {
            self.diagnostic_detail = true;
        } else {
            self.context_detail = Some(
                self.attention
                    .as_ref()
                    .map(|s| format!("{}\n\n{}\n\n{}\n{}", s.title, s.reason, s.impact, s.next))
                    .unwrap_or_else(|| "No recorded diagnostic available.".into()),
            );
        }
    }
    pub(super) fn recover_owner_action(&mut self, store: &Store) -> Result<()> {
        if self.runner.is_some() {
            self.notice = "Recovery refused: a local worker is active. No work resumed.".into();
            return Ok(());
        }
        let id = self.session().context("No selected session")?.id.clone();
        match store.recover_session(&id) {
            Ok(_) => {
                self.clear_diagnostic();
                self.refresh(store)?;
                self.notice="Owner recovery check finished. No work resumed; review recorded state and unfinished writes.".into();
            }
            Err(error) => {
                self.notice = format!("Owner recovery refused: {error:#}. No work resumed.")
            }
        }
        Ok(())
    }
}
pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    area: ratatui::layout::Rect,
    state: Option<&State>,
) {
    let diagnostic = app
        .diagnostic
        .as_deref()
        .unwrap_or("No diagnostic available.");
    let (heading, title, reason, impact, next, color) = if let Some(state) = state {
        (
            "Needs attention",
            state.title.as_str(),
            state.reason.as_str(),
            state.impact,
            state.next,
            state.color(),
        )
    } else {
        (
            "Needs attention",
            "Inspect the recorded diagnostic",
            diagnostic,
            "The operation did not finish successfully.",
            "Open the full diagnostic before deciding what to do next.",
            theme::ERROR,
        )
    };
    let width = area.width.saturating_sub(4) as usize;
    if width == 0 {
        return;
    }
    let mut lines = vec![
        Line::styled(
            heading,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Line::default(),
        Line::styled(
            format!(" {}", super::view::fit(title, width)),
            Style::default()
                .fg(color)
                .bg(theme::PANEL)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    let mut chars = reason.chars();
    let preview = chars
        .by_ref()
        .take(width.saturating_mul(4).max(256))
        .collect::<String>();
    let truncated = chars.next().is_some();
    let mut wrapped = vec![String::new()];
    for word in safe_display(&preview).split_whitespace() {
        let current = wrapped.last_mut().unwrap();
        if !current.is_empty()
            && unicode_width::UnicodeWidthStr::width(current.as_str())
                + 1
                + unicode_width::UnicodeWidthStr::width(word)
                > width
        {
            wrapped.push(String::new());
        }
        let current = wrapped.last_mut().unwrap();
        if !current.is_empty() {
            current.push(' ')
        }
        current.push_str(word);
    }
    for line in wrapped.iter().take(3) {
        lines.push(Line::styled(
            format!(" {}", super::view::fit(line, width)),
            Style::default().bg(theme::PANEL),
        ));
    }
    if wrapped.len() > 3 || truncated {
        lines.push(Line::styled(
            " Excerpt · open full evidence for details",
            Style::default().fg(theme::MUTED).bg(theme::PANEL),
        ));
    }
    for line in lines.iter_mut().skip(2) {
        let used = unicode_width::UnicodeWidthStr::width(line.to_string().as_str());
        line.spans.push(Span::raw(
            " ".repeat((area.width.saturating_sub(2) as usize).saturating_sub(used)),
        ));
    }
    lines.extend([
        Line::default(),
        Line::styled(
            "What this means",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(impact),
        Line::default(),
        Line::styled("Next", Style::default().add_modifier(Modifier::BOLD)),
        Line::styled(next, Style::default().fg(theme::MUTED)),
    ]);
    draw_scrolled(
        frame,
        app,
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn fixture() -> (tempfile::TempDir, Store, Session, App) {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut session = store
            .create("fix-bug", "Investigate the startup delay", "investigate")
            .unwrap();
        store
            .event(&session, Some("investigate"), "step.started", "")
            .unwrap();
        store
            .event(
                &session,
                Some("investigate"),
                "step.blocked",
                "Host-service measurement is unavailable; user logs are needed.",
            )
            .unwrap();
        session.status = "blocked".into();
        store.save(&session).unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        (root, store, session, app)
    }
    fn failure(store: &Store, session: &Session, purpose: &str, kind: ProviderKind) -> i64 {
        let failure = Failure::local(kind);
        store.event(session,Some(&session.current_step),"provider.failure",&serde_json::json!({"call_id":"coding-call","purpose":purpose,"request_number":1,"failure":failure,"untrusted_message":"fixture-secret-not-for-default-ui"}).to_string()).unwrap()
    }
    fn state(store: &Store, session: &Session, local: bool) -> Option<State> {
        let history = store.history(session).unwrap();
        let workspace =
            crate::workspace_view::SessionWorkspace::load(store, session, &history).unwrap();
        let report = store
            .usage_report(session, crate::usage::Scope::Session)
            .unwrap();
        project(session, &workspace, &history, local, &report.calls)
    }
    #[test]
    fn provider_categories_are_typed_and_old_or_manager_failures_do_not_become_current_cause() {
        let (_root, store, session, mut app) = fixture();
        failure(&store, &session, "coding", ProviderKind::Billing);
        app.refresh(&store).unwrap();
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Provider);
        assert!(app.attention.as_ref().unwrap().title.contains("billing"));
        assert!(!app
            .attention
            .as_ref()
            .unwrap()
            .reason
            .contains("fixture-secret"));
        // A read-only manager dispatch must not hide the coding failure.
        let mut call = crate::usage::Call::new(
            &session,
            &session.current_step,
            "manager",
            "fixture",
            "http://fixture",
            Default::default(),
            &[],
        );
        store.begin_model_call(&session, &mut call).unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Provider);
        store
            .event(
                &session,
                Some(&session.current_step),
                "model.call-started",
                "new-coding-call",
            )
            .unwrap();
        store
            .event(
                &session,
                Some(&session.current_step),
                "model.output",
                "action: finish\nsummary: done",
            )
            .unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Blocked);
        store
            .event(&session, Some(&session.current_step), "step.started", "")
            .unwrap();
        failure(&store, &session, "manager", ProviderKind::Credentials);
        app.refresh(&store).unwrap();
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Blocked);
    }
    #[test]
    fn retry_wait_requires_a_local_current_retry_and_disappears_on_the_next_dispatch() {
        let (_root, store, session, _app) = fixture();
        let owner = store.claim_execution(&session.id, None).unwrap();
        let current = store.session(&session.id).unwrap();
        failure(&store, &current, "coding", ProviderKind::RateLimit);
        store.event(&current,Some(&current.current_step),"provider.retry",&serde_json::json!({"call_id":"coding-call","purpose":"coding","next_request":2,"wait_ms":1000}).to_string()).unwrap();
        assert_eq!(state(&store, &current, true).unwrap().kind, Kind::Retry);
        assert_eq!(
            state(&store, &current, false).unwrap().kind,
            Kind::Observing
        );
        store
            .event(
                &current,
                Some(&current.current_step),
                "model.call-started",
                "next-call",
            )
            .unwrap();
        assert!(state(&store, &current, true).is_none());
        drop(owner);
    }
    #[test]
    fn recover_button_refuses_a_live_owner_then_recovers_without_resuming_or_replaying_approvals() {
        let (root, store, session, mut app) = fixture();
        let owner = store.claim_execution(&session.id, None).unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Observing);
        let before = store.history(&session).unwrap().len();
        app.activate(shell::Action::RecoverOwner, root.path(), &store)
            .unwrap();
        assert!(app.notice.contains("refused"));
        assert_eq!(store.history(&session).unwrap().len(), before);
        drop(owner);
        app.refresh(&store).unwrap();
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Owner);
        app.activate(shell::Action::RecoverOwner, root.path(), &store)
            .unwrap();
        assert!(app.runner.is_none());
        assert!(app.approval.is_none());
        assert_eq!(store.session(&session.id).unwrap().status, "blocked");
        assert_eq!(app.attention.as_ref().unwrap().kind, Kind::Recovered);
        assert_eq!(
            store
                .history(&session)
                .unwrap()
                .iter()
                .filter(|e| e.2 == "runner.started")
                .count(),
            1
        );
        assert_eq!(
            store
                .usage_report(&session, crate::usage::Scope::Session)
                .unwrap()
                .totals
                .requests,
            0
        );
    }
    #[test]
    fn write_guidance_never_executes_reconciliation_and_partial_restore_does_not_claim_rollback() {
        let (root, store, session, mut app) = fixture();
        let history = store.history(&session).unwrap();
        let mut workspace = app.workspace.clone().unwrap();
        workspace.primary = crate::workspace_view::PrimaryAction::RecoverWrites;
        app.attention = project(&session, &workspace, &history, false, &[]);
        app.blocker_card = true;
        app.clear_diagnostic();
        assert!(app.footer_actions().contains(&shell::Action::WriteGuidance));
        assert!(!app
            .footer_actions()
            .contains(&shell::Action::GiveInstructions));
        let before = store.history(&session).unwrap().len();
        app.activate(shell::Action::WriteGuidance, root.path(), &store)
            .unwrap();
        assert!(app
            .context_detail
            .as_ref()
            .unwrap()
            .contains("greenlight recover-writes"));
        assert_eq!(store.history(&session).unwrap().len(), before);
        assert!(app.runner.is_none());
        store.event(&session,None,"workspace.restore-failed",&serde_json::json!({"changed_files":2,"journaled_files":1,"path":"b.txt","error":"Second file failed after first update"}).to_string()).unwrap();
        let projected = state(&store, &session, false).unwrap();
        assert_eq!(projected.kind, Kind::Restore);
        assert!(projected.impact.contains("Some files may already"));
        assert!(!projected.resumable);
    }
    #[test]
    fn full_diagnostics_keep_evidence_and_help_does_not_trigger_a_recovery_action() {
        let (root, store, _session, mut app) = fixture();
        app.raise_diagnostic("A diagnostic with full details\nsecond line\nthird line".into());
        app.activate(shell::Action::FullDiagnostic, root.path(), &store)
            .unwrap();
        assert!(app.diagnostic_detail);
        assert!(app.export_snapshot().unwrap().text.contains("third line"));
        app.on_key(
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
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
        assert!(app.diagnostic_detail);
        assert!(app.runner.is_none());
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
        app.activate(shell::Action::RecoverOwner, root.path(), &store)
            .unwrap();
        assert!(received.try_recv().is_err());
        assert!(app.notice.contains("refused"));
    }
}
