//! Shared actions, hit regions and terminal input ownership.
use super::*;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::cell::RefCell;

pub(super) use super::actions::Action;

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
            self.approval_visible(),
            self.help,
            self.shell.sessions,
            self.artifact().map(|a| a.event_id),
            self.viewport,
            self.result_open,
            self.diagnostic.is_some(),
            self.context_detail.is_some(),
            self.details.page.is_some(),
            self.edits.pending.is_some(),
            self.restart_plan.is_some(),
            self.new_session.is_some(),
            self.searching,
            self.activity_filtering,
            self.artifact_browser.is_some()
        ) + &format!(
            ":{}:{}:{:?}",
            self.shell.more, self.shell.menu_index, self.shell.actions_focus
        ) + &format!(
            ":{:?}:{:?}:{:?}",
            self.details.page,
            self.details.selected,
            self.details
                .page
                .and(self.details.report.as_ref())
                .map(|r| r.calls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>())
        ) + &format!(
            ":{:?}:{}:{}",
            self.compose.mode, self.checkpoint.reading, self.checkpoint.index
        ) + &format!(
            ":{}:{:?}:{:?}:{:?}",
            self.edits.overview,
            self.edits.hunk,
            self.navigation.selected,
            self.navigation
                .candidates
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
        ) + &format!(
            ":{}:{}:{}",
            self.navigation.error.is_some(),
            self.navigation.load_error.is_some(),
            self.navigation.missing.is_some()
        ) + &format!(
            ":{:?}",
            self.sessions
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
        ) + &format!(
            ":{:?}",
            self.edits
                .files
                .iter()
                .map(|f| (&f.path, f.revision, f.kept))
                .collect::<Vec<_>>()
        )
    }
    pub(super) fn begin_frame(&self) {
        self.shell.hits.borrow_mut().clear();
        *self.shell.stamp.borrow_mut() = self.shell_stamp();
    }
    pub(super) fn blocking_surface(&self) -> bool {
        self.shell.sessions
            || self.navigation.error.is_some()
            || self.navigation.load_error.is_some()
            || self.details.page.is_some()
            || self.help
            || self.approval_visible()
            || self.shell.more
            || self.searching
            || self.activity_filtering
            || self.new_session.is_some()
            || self.diagnostic.is_some()
            || self.context_detail.is_some()
            || self.restart_plan.is_some()
            || !self.restart_choices.is_empty()
            || self.edits.pending.is_some()
            || self.artifact_browser.is_some()
    }
    pub(super) fn workflow_live_output(&self) -> Option<&crate::provider::stream::Preview> {
        let session = self.session()?;
        self.runner
            .as_ref()
            .filter(|worker| worker.session_id.as_deref() == Some(session.id.as_str()))
            .and_then(|worker| worker.live.as_ref())
    }
    pub(super) fn composer_active(&self) -> bool {
        self.focus == Focus::Compose && !self.blocking_surface()
    }
    pub(super) fn more_actions(&self) -> Vec<Action> {
        if self.diagnostic.is_some() || self.attention_visible() {
            let mut actions = vec![
                Action::RecordedActivity,
                Action::Conversation,
                Action::AskManager,
                Action::FullDiagnostic,
                Action::Provider,
                Action::ConversationDump,
            ];
            if (self.diagnostic.is_none() || self.diagnostic_attention)
                && self.attention.as_ref().is_some_and(|s| s.resumable)
            {
                actions.push(Action::RestartReview);
            }
            if self.session().is_some() {
                actions.extend([
                    Action::CompareBranches,
                    Action::GenerateSummary,
                    Action::LatestSummary,
                ]);
            }
            if self.summary_running() {
                actions.push(Action::CancelSummary);
            }
            return actions;
        }

        if self.artifact_browser.is_some() {
            let mut actions = vec![
                Action::EditArtifact,
                Action::CommentArtifact,
                Action::ArtifactComments,
                Action::ConversationDump,
                Action::Versions,
                Action::Provenance,
                Action::Compare,
                Action::Copy,
                Action::Editor,
            ];
            if self.session().is_some_and(|s| s.status == "completed") {
                actions.insert(0, Action::Overview);
            }
            return actions;
        }

        if self.session().is_none() {
            return vec![Action::New, Action::Sessions, Action::Help];
        }
        let mut actions = vec![
            Action::Activity,
            Action::Artifacts,
            Action::Edits,
            Action::New,
            Action::Usage,
            Action::Provider,
            Action::ConversationDump,
            Action::ExportVerification,
            Action::RouteStatus,
        ];
        if self.session().is_some_and(|s| s.status == "completed")
            && !(self.result_open && self.view == View::Conversation)
        {
            actions.insert(0, Action::Overview);
            if self.view == View::Conversation {
                actions.push(Action::Compose);
            }
        }
        if self.view == View::Edits && !self.edits.files.is_empty() {
            actions.splice(0..0, [Action::OpenEdit, Action::EditAll, Action::RevertAll]);
            if self.edits.has_hunks() {
                actions.extend([Action::PreviousHunk, Action::NextHunk, Action::FullDiff]);
            }
        }
        if self.focus != Focus::Compose {
            actions.extend([Action::Copy, Action::Editor]);
        }
        if self.view == View::Conversation {
            actions.extend([Action::Filter, Action::Follow]);
        }
        if self.saved_workflow_request && self.runner.is_none() {
            actions.push(Action::RetrySaved);
        }
        if self.saved_manager_request && self.manager_job.is_none() {
            actions.push(Action::RetrySavedManager);
        }
        if self.runner.is_some() || self.manager_job.is_some() {
            actions.push(Action::Stop);
        }
        if self.runner.is_none()
            && self.manager_job.is_none()
            && self.selected_tool_request().is_some()
        {
            actions.push(Action::RerunTool);
        }
        if self.result_open && self.view == View::Conversation {
            actions.insert(0, Action::ResultEvidence);
        }
        if self.view == View::Artifacts && self.artifact().is_some() {
            actions.extend([
                Action::EditArtifact,
                Action::CommentArtifact,
                Action::ArtifactComments,
                Action::Versions,
                Action::Provenance,
                Action::Compare,
            ]);
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
            actions.extend([
                Action::CompareBranches,
                Action::GenerateSummary,
                Action::LatestSummary,
                Action::Conversation,
                Action::AskManager,
                Action::ManagerReplies,
            ]);
        }
        if self.summary_running() {
            actions.push(Action::CancelSummary);
        }
        actions.push(Action::Sessions);
        if self.session().is_some() {
            actions.push(Action::Details);
        }
        actions.push(Action::Help);
        actions.extend([Action::Preview, Action::Expand]);
        actions
    }
    pub(super) fn header_actions(&self) -> Vec<Action> {
        if self.blocking_surface() || self.session_total == 0 {
            vec![Action::Help]
        } else if self.session().is_none() {
            vec![Action::Sessions, Action::Help]
        } else {
            vec![Action::Sessions, Action::Details, Action::Help]
        }
    }
    fn selected_route_event(&self) -> Option<i64> {
        let event = self.event()?;
        if event.kind != "manager.result" {
            return None;
        }
        let reply: crate::manager::Reply = serde_json::from_str(&event.data).ok()?;
        (matches!(reply.decision, crate::manager::Decision::Forward { .. })
            && !reply.snapshot_changed)
            .then_some(event.id)
    }
    fn selected_route_message(&self) -> Option<String> {
        let event = self.event()?;
        if event.kind != "manager.delivery" {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(&event.data).ok()?;
        if !matches!(
            value["state"].as_str(),
            Some("queued" | "needs_review" | "restart_proposed")
        ) {
            return None;
        }
        value["message_id"].as_str().map(str::to_owned)
    }
    pub(super) fn selected_tool_request(&self) -> Option<i64> {
        if self.view != View::Conversation {
            return None;
        }
        let event = self.event()?;
        if event.kind == "tool.request" {
            return Some(event.id);
        }
        self.activity_cards
            .tools
            .get(&event.id)
            .map(|e| e.metadata.request_event)
    }
    pub(super) fn footer_actions(&self) -> Vec<Action> {
        let mut actions = self.base_footer_actions();
        if !self.reading_overlay() && !self.composer_active() && self.view == View::Conversation {
            if self.event().is_some_and(|e| {
                e.kind == "manager.result"
                    && serde_json::from_str::<crate::manager::Reply>(&e.data).is_ok_and(|r| {
                        matches!(r.decision, crate::manager::Decision::Clarify { .. })
                    })
            }) {
                actions.insert(0, Action::ReplyClarification);
            }
            if self.selected_route_event().is_some() {
                actions.insert(0, Action::DeliverRoute);
            }
            if self.selected_route_message().is_some() {
                actions.insert(0, Action::WithdrawRoute);
            }
            if self.event().is_some_and(|e| {
                e.kind == "manager.delivery"
                    && serde_json::from_str::<serde_json::Value>(&e.data)
                        .ok()
                        .is_some_and(|v| {
                            matches!(
                                v["state"].as_str(),
                                Some("needs_review" | "restart_proposed")
                            )
                        })
            }) {
                actions.insert(0, Action::ReviewGuidanceRestart);
            }
            if self
                .session()
                .is_some_and(|s| s.status == "instruction_review")
            {
                actions.insert(0, Action::ContinueGuidance);
            }
        }
        if self.approval.is_some() && self.approval_hidden && !self.help && !self.shell.more {
            actions.insert(
                0,
                if self.provider_retry_pending() {
                    Action::ReviewProviderRetry
                } else {
                    Action::ReviewPermission
                },
            );
        }
        if self.project_attention() && !self.reading_overlay() && !self.composer_active() {
            actions.insert(0, Action::RefreshProject);
            if self.other_project_owner()
                && self.project.as_ref().is_some_and(|p| p.session.is_some())
            {
                actions.insert(0, Action::OpenProjectOwner);
            }
        }
        actions
    }
    fn base_footer_actions(&self) -> Vec<Action> {
        if self.help {
            return vec![Action::Close, Action::Mouse];
        }
        if self.permission_popup_visible() {
            return vec![];
        }
        if self.approval_visible() {
            if self.human_review().is_some() {
                return if self.checkpoint.reading {
                    vec![Action::ReviewBack, Action::Copy, Action::Help]
                } else {
                    vec![Action::ReviewOpen, Action::Help]
                };
            }
            return vec![Action::Help];
        }
        if self.shell.sessions {
            if self.navigation.error.is_some() {
                return vec![Action::RetryDiscovery, Action::CloseNavigator];
            }
            let mut actions = Vec::new();
            if self.navigation.candidate().is_some() {
                actions.push(Action::OpenCandidate);
            }
            if !self.session_query.is_empty() {
                actions.push(Action::ClearSearch);
            }
            if self.session_total == 0 {
                actions.push(Action::New);
            }
            actions.push(Action::CloseNavigator);
            return actions;
        }
        if self.navigation.error.is_some() {
            return vec![Action::RetryDiscovery, Action::Help];
        }
        if self.navigation.load_error.is_some() {
            return vec![Action::Sessions, Action::RetryDiscovery, Action::Help];
        }
        if self.navigation.missing.is_some() {
            return vec![Action::Sessions, Action::New, Action::Help];
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
            if matches!(
                self.compose.mode,
                compose::Mode::Manager | compose::Mode::Conversation
            ) && self.manager_job.is_some()
            {
                return vec![Action::StopManager, Action::Recipient, Action::Close];
            }
            if self.compose.mode == compose::Mode::Step && !self.can_resume_message() {
                return vec![Action::Recipient, Action::Commands, Action::Close];
            }
            let send = match self.compose.mode {
                compose::Mode::Step => Action::SendStep,
                compose::Mode::Manager => Action::SendQuestion,
                compose::Mode::Conversation => Action::SendConversation,
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
            if self.session().is_some_and(|s| s.status == "completed") {
                return if self.viewport.0 < 60 {
                    vec![Action::Overview, Action::Close, Action::More]
                } else {
                    vec![
                        Action::Versions,
                        Action::Provenance,
                        Action::Overview,
                        Action::Close,
                    ]
                };
            }
            return vec![Action::Versions, Action::Provenance, Action::Close];
        }
        if self.diagnostic.is_some() {
            if self.diagnostic_detail {
                return vec![Action::Close, Action::Copy];
            }
            if self.diagnostic_attention {
                if let Some(state) = &self.attention {
                    return state.actions();
                }
            }
            return vec![Action::FullDiagnostic, Action::Close];
        }
        if self.details.page.is_some() {
            return self.details.actions();
        }
        if self.edits.pending.is_some() {
            return vec![Action::CancelRevert, Action::ConfirmRevert, Action::Help];
        }
        if self.blocking_surface() {
            return vec![Action::Help, Action::Close];
        }
        if self.session().is_none() {
            return vec![Action::New, Action::More];
        }
        if self.saved_workflow_request
            && self.runner.is_none()
            && self.manager_job.is_none()
            && !self.attention.as_ref().is_some_and(|state| {
                matches!(
                    state.kind,
                    attention::Kind::Owner | attention::Kind::Writes | attention::Kind::Observing
                )
            })
        {
            return vec![Action::RetrySaved, Action::InspectAttention, Action::More];
        }
        if self.attention_visible() {
            return self.attention.as_ref().unwrap().actions();
        }
        if self.result_open && self.view == View::Conversation {
            if self.viewport.0 < 60 {
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
                    Action::Overview,
                    Action::More,
                ];
            }
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
                Action::Overview,
                Action::More,
            ];
        }
        if self.view == View::Edits
            && self.edits.hunk.is_some()
            && !self.edits.overview
            && self.edits.pending.is_none()
            && self.edits.error.is_none()
        {
            return vec![
                Action::PreviousHunk,
                Action::NextHunk,
                Action::FullDiff,
                Action::More,
            ];
        }
        if self.view == View::Edits {
            if self.edits.error.is_some() || self.edits.files.is_empty() {
                return vec![Action::More];
            }
            let compact = self.viewport.0 < 60;
            if self.edits.overview {
                if !self.edits.files.iter().any(|f| f.changed() || f.gap) {
                    return vec![Action::OpenEdit, Action::More];
                }
                return if compact {
                    vec![Action::KeepAll, Action::RevertAll, Action::More]
                } else {
                    vec![
                        Action::KeepAll,
                        Action::RevertAll,
                        Action::OpenEdit,
                        Action::More,
                    ]
                };
            }
            if !self
                .edits
                .files
                .get(self.edits.index)
                .is_some_and(|f| f.changed() && !f.gap)
            {
                return vec![Action::EditAll, Action::More];
            }
            return if compact {
                vec![Action::KeepFile, Action::RevertFile, Action::More]
            } else {
                vec![
                    Action::KeepFile,
                    Action::RevertFile,
                    Action::EditAll,
                    Action::More,
                ]
            };
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
        if self.runner.is_some() && self.view == View::Conversation {
            return vec![
                if self.workflow_live_output().is_some()
                    && self.shell.preview
                    && (self.inspector_open || self.focus == Focus::Detail)
                {
                    Action::ReturnLive
                } else {
                    Action::Open
                },
                Action::Stop,
                Action::More,
            ];
        }
        if self.selected_tool_request().is_some()
            && self.runner.is_none()
            && self.manager_job.is_none()
        {
            return vec![Action::Open, Action::RerunTool, Action::More];
        }
        if self.session().is_some_and(|s| s.status == "completed") {
            return if self.viewport.0 < 60 {
                vec![Action::Open, Action::Overview, Action::More]
            } else {
                vec![
                    Action::Open,
                    Action::Compose,
                    Action::Overview,
                    Action::More,
                ]
            };
        }
        vec![
            if self.workflow_live_output().is_some()
                && self.shell.preview
                && (self.inspector_open || self.focus == Focus::Detail)
            {
                Action::ReturnLive
            } else {
                Action::Open
            },
            Action::Compose,
            Action::More,
        ]
    }
    pub(super) fn shell_actions(&self) -> Vec<Action> {
        let mut actions = self.footer_actions();
        if !self.blocking_surface()
            || self.new_session.is_some()
            || self.diagnostic.is_some()
            || self.details.page.is_some()
            || self.shell.sessions
        {
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
        use super::actions::DecisionLayer;
        use Action::*;
        if self.partial_history
            && self.view == View::Conversation
            && self.decision_layer() == DecisionLayer::Workspace
            && !self.reading_overlay()
            && matches!(action, Open | Copy | Editor)
        {
            if let Err(error) = self.load_selected_event(store) {
                self.raise_diagnostic(format!("Exact event unavailable: {error:#}"));
                return Ok(false);
            }
        }
        let allowed = match self.decision_layer() {
            DecisionLayer::Help => matches!(action, Close | Mouse | Copy),
            DecisionLayer::Permission => matches!(action, Close | Decision(_)),
            DecisionLayer::HumanEvidence => matches!(action, ReviewBack | Copy | Help | Close),
            DecisionLayer::HumanChoice => matches!(action, ReviewOpen | Help | Close | Decision(_)),
            DecisionLayer::Revert => matches!(
                action,
                ConfirmRevert | CancelRevert | Help | Close | Copy | Editor
            ),
            DecisionLayer::Workspace => !matches!(action, Decision(_)),
        };
        if !allowed {
            return Ok(false);
        }
        if self.shell.sessions
            && matches!(
                action,
                Activity | Artifacts | Edits | Event(_) | Artifact(_) | Edit(_) | Details | Compose
            )
        {
            self.close_navigator();
        }
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
        match action {
            ContinueGuidance => {
                let session = self.session().context("No selected session")?.id.clone();
                match store.continue_after_guidance(root, &session) {
                    Ok(()) => {
                        self.clear_diagnostic();
                        self.refresh(store)?;
                        self.notice="Review hold resolved using the recorded success edge. No step was repeated; resume the next ready step explicitly.".into();
                    }
                    Err(error) => self.notice = format!("Cannot continue: {error:#}"),
                }
            }
            ReviewGuidanceRestart => {
                let message = self
                    .selected_route_message()
                    .context("Select an undelivered instruction")?;
                if let Err(error) = self.prepare_manager_restart(root, store, &message) {
                    self.notice = format!("Restart review unavailable: {error:#}");
                }
            }
            RouteStatus => {
                let session = self.session().context("No session selected")?;
                let routes = store.manager_routes(session)?;
                self.context_detail = Some(if routes.is_empty() {
                    "Instruction delivery · no accepted routes on this branch. Select a manager forward proposal and use Deliver original. Ask/Answer alone never executes it.".into()
                } else {
                    format!("Instruction delivery · current branch snapshot\nQueued means not included yet. Reopen to refresh.\n\n{}",routes.iter().map(|r|r.display()).collect::<Vec<_>>().join("\n\n"))
                });
                self.detail_scroll = 0;
            }
            DeliverRoute => {
                let event = self
                    .selected_route_event()
                    .context("Select a manager forward proposal")?;
                if let Err(error) = self.deliver_manager_result(root, store, event) {
                    self.notice = format!("Route not delivered: {error:#}. Original retained.");
                }
            }
            WithdrawRoute => {
                let message = self
                    .selected_route_message()
                    .context("Select a manager delivery record")?;
                let session = self.session().context("No session")?.id.clone();
                self.notice = match store.withdraw_manager_route(&session, &message) {
                    Ok(()) => "Instruction withdrawn; original evidence retained.".into(),
                    Err(error) => format!("Cannot withdraw: {error:#}"),
                };
                self.refresh(store)?;
            }
            OpenProjectOwner => {
                if let Some(owner) = store.project_status()?.and_then(|p| p.session) {
                    self.remember_creation();
                    self.new_session = None;
                    self.requested_session = Some(owner.id);
                    self.shell.sessions = false;
                    self.clear_diagnostic();
                    self.refresh(store)?;
                    self.notice = "Opened project owner. Other clients' approval/input waits may be unavailable here; browsing does not stop the worker.".into();
                } else {
                    self.refresh(store)?;
                    self.notice = "Project owner changed; refreshed.".into();
                }
            }
            RefreshProject => {
                self.refresh(store)?;
                self.notice = self
                    .project
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| {
                        "Project available. Start explicitly when ready; no work was queued.".into()
                    });
            }
            RetrySaved => {
                if let Err(error) = store.ensure_project_available() {
                    self.notice = format!("Not started: {error:#}");
                    return Ok(false);
                }
                if self.runner.is_some() || self.manager_job.is_some() {
                    self.notice = "Stop active execution before retrying the saved request.".into();
                    return Ok(false);
                }
                let session = self.session().context("No selected session")?;
                if store.execution_state(&session.id)?
                    != crate::session_execution::ExecutionState::Idle
                {
                    self.notice =
                        "Recover the interrupted owner before retrying the saved request.".into();
                    return Ok(false);
                }
                if store.pending_continuation(session, false)?.is_none() {
                    self.refresh(store)?;
                    return Ok(false);
                }
                self.runner = Some(tui_actions::resume_saved(
                    root.to_path_buf(),
                    session.id.clone(),
                ));
                self.clear_diagnostic();
                self.result_open = false;
                self.notice = "Validating saved request and workspace. Exact messages and attempt will be retained; earlier tools will not run again. Credentials reload from current settings; remote billing may be uncertain.".into();
            }
            RetrySavedManager => {
                self.retry_saved_manager(root, store);
            }
            ResumeProvider => {
                self.refresh(store)?;
                if self.saved_workflow_request {
                    return self.activate(RetrySaved, root, store);
                }
                if !self
                    .attention
                    .as_ref()
                    .is_some_and(|s| s.kind == attention::Kind::Provider && s.resumable)
                {
                    self.notice="Provider recovery state changed; inspect the current session before continuing.".into();
                    return Ok(false);
                }
                self.clear_diagnostic();
                let draft = self.composer.clone();
                let last_instruction = self.compose.last_instruction.clone();
                self.resume_instruction(root, store, String::new());
                self.composer = draft;
                self.compose.last_instruction = last_instruction;
            }
            ReviewProviderRetry | ReviewPermission => {
                self.reopen_permission();
            }
            InspectAttention => {
                self.inspect_attention();
            }
            ConversationDump => {
                self.diagnostic = None;
                self.diagnostic_attention = false;
                self.open_conversation_dump(store);
            }
            CopyDump => {
                self.start_copy();
            }
            RefreshDump => {
                self.reload_conversation_dump(store);
            }
            SaveDump => {
                self.save_conversation_dump(store);
            }
            FullDiagnostic => {
                if self.diagnostic_attention
                    || self.diagnostic.is_none() && self.attention_visible()
                {
                    if let Some(state) = &self.attention {
                        let record = state
                            .evidence
                            .and_then(|id| self.activity_rows.iter().find(|e| e.id == id));
                        self.diagnostic_revision = self.diagnostic_revision.wrapping_add(1);
                        self.diagnostic = Some(format!(
                            "{}\n\n{}\n\n{}",
                            state.title,
                            state.reason,
                            record
                                .map(|e| format!(
                                    "Recorded event #{} · {} · {}\n{}",
                                    e.id, e.step, e.kind, e.data
                                ))
                                .unwrap_or_else(|| state.impact.to_owned())
                        ));
                    }
                }
                if self.diagnostic.is_some() {
                    self.diagnostic_detail = true;
                    self.detail_scroll = 0;
                } else {
                    self.inspect_attention();
                }
            }
            RecordedActivity => {
                self.attention_history();
            }
            GiveInstructions => {
                self.clear_diagnostic();
                self.set_message_mode(compose::Mode::Step);
                self.focus_composer();
            }
            ReplyClarification => {
                let event = self.event().context("Select a clarification")?.id;
                let raw = &self.event().context("Select a clarification")?.data;
                let reply: crate::manager::Reply = serde_json::from_str(raw)?;
                if !matches!(reply.decision, crate::manager::Decision::Clarify { .. }) {
                    bail!("Selected result is not a clarification");
                }
                let scope = self.session().context("No selected session")?.clone();
                let automatic = store.history(&scope)?.iter().any(|e| {
                    e.2 == "manager.routing-requested"
                        && serde_json::from_str::<serde_json::Value>(&e.3).is_ok_and(|v| {
                            v["message_id"].as_str() == Some(reply.decision.message_id())
                        })
                });
                self.clear_diagnostic();
                self.set_message_mode(if automatic {
                    compose::Mode::Conversation
                } else {
                    compose::Mode::Manager
                });
                self.compose
                    .clarification
                    .insert(self.compose.scope.clone(), event);
                self.focus_composer();
                self.notice="Reply to the selected clarification. Original request is preserved; the visible recipient determines whether a validated forward can be delivered.".into();
            }
            Conversation => {
                self.clear_diagnostic();
                self.set_message_mode(compose::Mode::Conversation);
                self.focus_composer();
            }
            CancelSummary => self.cancel_manager(),
            GenerateSummary => {
                self.start_summary(root)?;
            }
            LatestSummary => {
                let session = self.session().context("Select a session")?;
                let summary = store.latest_summary(session)?.context(
                    "No recorded summary on this branch. Use Generate summary to request one.",
                )?;
                self.context_detail = Some(summary.display());
                self.detail_scroll = 0;
            }
            AskManager => {
                self.clear_diagnostic();
                self.set_message_mode(compose::Mode::Manager);
                self.compose.clarification.remove(&self.compose.scope);
                self.focus_composer();
            }
            RecoverOwner => {
                self.recover_owner_action(store)?;
            }
            WriteGuidance => {
                self.clear_diagnostic();
                self.context_detail=Some(format!("Native-write recovery\n\nProject: {}\n\nFrom this project, run:\n  greenlight recover-writes\n\nThis reconciles recorded write intents with current disk contents; it does not overwrite source files. Read the CLI result before resuming. Recovery can refuse drift or an active native-write lock.\n\nOwner recovery (/recover) is a separate operation and never resumes work.",root.display()));
                self.detail_scroll = 0;
            }
            CompareBranches => {
                let session = self.session().context("Select a session")?;
                let branches = store.branches(&session.id)?;
                if branches.len() > store.limits.history_events {
                    bail!("Branch count exceeds configured history limit");
                }
                let mut report=String::from("Compare recorded branches against the selected branch. No files or session state are changed.\n\n");
                for branch in branches.iter().filter(|b| b.id != session.active_branch) {
                    report.push_str(&tui_actions::compare_branches(
                        store,
                        session,
                        &session.active_branch,
                        &branch.id,
                    )?);
                    report.push('\n');
                    crate::resource_limits::Budget::new(store.limits.materialized_bytes)
                        .charge(report.len())?;
                }
                if branches.len() < 2 {
                    report.push_str("No other branches recorded for this session.");
                }
                self.context_detail = Some(report);
                self.detail_scroll = 0;
            }
            RestartReview => {
                self.clear_diagnostic();
                self.open_restart(root, store);
            }
            SendStep | SendQuestion | SendConversation | SendCommand => {
                let expected = match action {
                    SendStep => compose::Mode::Step,
                    SendQuestion => compose::Mode::Manager,
                    SendConversation => compose::Mode::Conversation,
                    _ => compose::Mode::Command,
                };
                if self.compose.mode != expected || self.focus != Focus::Compose {
                    bail!("Message target changed; review the recipient before sending")
                }
                self.send_message(root, store);
            }
            StopManager => {
                self.cancel_manager();
            }
            Commands => {
                self.set_message_mode(compose::Mode::Command);
                self.focus_composer();
                if self.composer.is_empty() {
                    self.composer.push('/');
                }
            }
            ManagerReplies => {
                self.result_open = false;
                self.view = View::Conversation;
                self.activity_query = "manager.".into();
                self.filter_activity();
                self.commit_activity_filter();
                self.focus = Focus::Events;
                self.follow_activity = false;
            }
            Recipient => {
                self.choose_recipient();
            }
            RestoreInstruction => {
                self.restore_instruction();
            }
            Continue | StartWorkflow => {
                self.submit_new_session(root, store)?;
            }
            Cancel | Close => {
                self.close_surface(store)?;
            }
            ClearGoal => {
                self.compose.creation_goal.clear();
                if let Some(form) = &mut self.new_session {
                    form.goal = Some(String::new());
                    form.error = None;
                }
                self.goal_editor.reset();
            }
            BackWorkflow => {
                self.remember_creation();
                if let Some(form) = &mut self.new_session {
                    form.goal = None;
                    form.error = None;
                }
            }
            Workflow(index) => {
                if let Some(form) = &mut self.new_session {
                    if index < form.workflows.len() {
                        form.cursor = index;
                        form.error = None;
                    }
                }
            }
            ResultEvidence => {
                self.context_detail = Some(self.result_text.clone());
                self.detail_scroll = 0;
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
            }
            Overview => {
                if self.session().is_some_and(|s| s.status == "completed")
                    && !self.approval_visible()
                    && self.edits.pending.is_none()
                    && self.restart_plan.is_none()
                {
                    self.artifact_browser = None;
                    self.context_detail = None;
                    self.details.close();
                    self.switch_view(View::Conversation);
                    self.result_open = true;
                    self.focus = Focus::Detail;
                    self.detail_scroll = 0;
                }
            }
            ReviewEdits => {
                self.switch_view(View::Edits);
                self.refresh_edits(store);
            }
            PreviousHunk | NextHunk => self.navigate_hunk(action == PreviousHunk),
            FullDiff => {
                self.edits.hunk = None;
                self.detail_scroll = 0;
                self.rendered_scroll.set(None);
                self.notice = "Full recorded file diff; no files changed.".into();
            }
            ExportVerification => self.export_verification(store),
            EditArtifact | CommentArtifact => {
                if let Err(error) = self.prepare_artifact_review(store, action == CommentArtifact) {
                    self.notice = format!("Artifact review unavailable: {error:#}");
                }
            }
            ArtifactComments => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Comments)?;
            }
            Versions => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Content)?;
            }
            Provenance => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Provenance)?;
            }
            Compare => {
                self.open_artifact_mode(store, crate::artifact_history::Mode::Diff)?;
            }
            Sessions => {
                if self.shell.sessions {
                    self.close_navigator();
                } else {
                    self.open_navigator(true);
                }
            }
            OpenCandidate => {
                self.open_candidate(store)?;
            }
            CloseNavigator => {
                self.close_navigator();
            }
            ClearSearch => {
                self.session_query.clear();
                self.session_editor.reset();
                self.refresh(store)?;
            }
            NavSearch => {
                self.searching = true;
                self.session_editor.reset();
            }
            RetryDiscovery => {
                self.refresh(store)?;
            }
            Compose => {
                if self.shell.sessions {
                    self.close_navigator();
                }
                self.focus_composer();
            }
            Details => {
                self.open_run_details(details::Page::Run, store);
            }
            Help => {
                self.open_help();
            }
            More => {
                self.shell.more = true;
                self.shell.menu_index = 0;
            }
            Mouse => {
                self.shell.mouse = !self.shell.mouse;
            }
            Activity | Artifacts | Edits => {
                self.switch_view(match action {
                    Activity => View::Conversation,
                    Artifacts => View::Artifacts,
                    _ => View::Edits,
                });
                if action == Edits {
                    self.refresh_edits(store);
                }
            }
            New => {
                if self.shell.sessions {
                    self.close_navigator();
                }
                if self.runner.is_some() {
                    self.notice =
                        "Wait for the active worker before creating another session.".into();
                } else if let Err(error) = self.new_form(root) {
                    self.raise_diagnostic(format!("Cannot list workflows: {error:#}"));
                }
            }
            ReturnLive => {
                self.inspector_open = false;
                self.focus = Focus::Events;
                self.detail_scroll = 0;
            }
            Open => {
                self.open_selection(root, store)?;
            }
            Copy => {
                self.start_copy();
            }
            Editor => {
                self.request_editor();
            }
            Filter => {
                if self.view == View::Conversation {
                    self.activity_filtering = true;
                    self.filter_editor.reset();
                }
            }
            ConfirmRevert => {
                if !self.too_small() {
                    self.confirm_revert(root, store);
                }
            }
            CancelRevert => {
                self.edits.pending = None;
                self.detail_scroll = 0;
                self.notice = "Revert cancelled; no files changed.".into();
            }
            OpenEdit => {
                self.edits.hunk = None;
                self.edits.overview = false;
                self.edits.select(0);
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
            }
            KeepFile => {
                self.keep_edits(root, store, false);
            }
            KeepAll => {
                self.keep_edits(root, store, true);
            }
            RevertFile => {
                self.prepare_revert(root, store, false);
            }
            RevertAll => {
                self.prepare_revert(root, store, true);
            }
            EditAll => {
                self.edits.hunk = None;
                self.edits.overview = true;
                self.focus = Focus::Artifacts;
                self.detail_scroll = 0;
            }
            ReviewOpen => {
                self.checkpoint_open(store);
            }
            ReviewBack => {
                self.checkpoint_back(store);
            }
            Usage => {
                self.open_run_details(details::Page::Usage, store);
            }
            Provider => {
                self.clear_diagnostic();
                self.context_detail = None;
                self.open_run_details(details::Page::Provider, store);
            }
            Technical => {
                self.open_run_details(details::Page::Technical, store);
            }
            Requests => {
                self.open_run_details(details::Page::Requests, store);
            }
            UsageAccounting => {
                self.open_run_details(details::Page::Accounting, store);
            }
            DetailsBack => {
                self.back_run_details(store);
            }
            OpenRequest => {
                if self.details.selected_call().is_some() {
                    self.open_run_details(details::Page::Request, store);
                }
            }
            UsageRequest(index) => {
                if self.details.page == Some(details::Page::Requests) {
                    if let Some(call) = self
                        .details
                        .report
                        .as_ref()
                        .and_then(|r| r.calls.get(index))
                    {
                        self.details.selected = Some(call.id.clone());
                        self.open_run_details(details::Page::Request, store);
                    }
                }
            }
            Follow => {
                if self.focus == Focus::Detail {
                    self.detail_scroll = u16::MAX;
                } else {
                    self.select_end(true);
                }
            }
            RerunTool => {
                if self.runner.is_some() || self.manager_job.is_some() {
                    self.notice = "Stop execution before rerunning a tool.".into();
                    return Ok(false);
                }
                let request = self
                    .selected_tool_request()
                    .context("Select a recorded tool call first")?;
                let session = self.session().context("No selected session")?;
                if store.execution_state(&session.id)?
                    != crate::session_execution::ExecutionState::Idle
                {
                    self.notice = "Stop or recover the session before rerunning a tool.".into();
                    return Ok(false);
                }
                store.ensure_no_pending_writes()?;
                store.rerun_request(session, request)?;
                self.runner = Some(tui_actions::rerun(
                    root.to_path_buf(),
                    session.id.clone(),
                    session.active_branch.clone(),
                    request,
                ));
                self.result_open = false;
                self.shell.more = false;
                self.notice = "Running exact recorded tool arguments at your request. Partial effects may repeat; original history is preserved.".into();
            }
            Stop => {
                if let Some(cancel) = self
                    .runner
                    .as_ref()
                    .and_then(|runner| runner.cancel_retry.as_ref())
                {
                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    self.notice = "Stopping execution… Active requests/commands are being cancelled; applied edits remain. Provider billing or partial command effects may be uncertain.".into();
                } else if self.manager_job.is_some() {
                    self.cancel_manager();
                }
            }
            Preview => {
                self.shell.preview = !self.shell.preview;
            }
            Expand => {
                self.shell.expanded = !self.shell.expanded;
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
            }
            Decision(choice) => {
                if !self.too_small() {
                    self.deliver_checked_approval(choice, store)?;
                }
            }
            Event(id) => {
                self.attention_return = false;
                if let Some(i) = self.events.iter().position(|e| e.id == id) {
                    self.event_index = i;
                    self.focus = Focus::Detail;
                    self.inspector_open = true;
                    self.follow_activity = false;
                    self.detail_scroll = 0;
                    if let Err(error) = self.load_selected_event(store) {
                        self.raise_diagnostic(format!("Exact event unavailable: {error:#}"));
                    }
                }
            }
            Artifact(id) => {
                if let Some(i) = self.artifacts.iter().position(|a| a.event_id == id) {
                    self.artifact_index = i;
                    self.focus = Focus::Detail;
                    self.inspector_open = true;
                    self.detail_scroll = 0;
                }
            }
            Edit(i) => {
                if i < self.edits.files.len() {
                    self.edits.index = i;
                    self.edits.overview = false;
                    self.edits.select(0);
                    self.focus = Focus::Detail;
                    self.inspector_open = true;
                    self.detail_scroll = 0;
                }
            }
            Session(i) => {
                if self.shell.sessions && i < self.navigation.candidates.len() {
                    self.navigation.index = i;
                    self.navigation.selected = Some(self.navigation.candidates[i].id.clone());
                    self.searching = false;
                    self.focus = Focus::Sessions;
                }
            }
        }
        Ok(false)
    }
    pub(super) fn on_event(&mut self, event: Event, root: &Path, store: &Store) -> Result<bool> {
        let result = self.route_event(event, root, store);
        self.load_selected_artifact(store);
        result
    }
    fn route_event(&mut self, event: Event, root: &Path, store: &Store) -> Result<bool> {
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
                    || self.approval_visible()
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
                if hit.is_none() && !self.help && !self.approval_visible() {
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
            | Action::InspectAttention
            | Action::RecoverOwner
            | Action::WriteGuidance
            | Action::SendStep
            | Action::SendQuestion
            | Action::SendCommand
            | Action::Continue
            | Action::StartWorkflow
    );
    // Primary action emphasis is independent from keyboard focus.
    let keyboard_focus = focused;
    let active_overview = action == Action::Overview
        && app.result_open
        && app.view == View::Conversation
        && !app.blocking_surface();
    let base = if keyboard_focus {
        Style::default()
            .fg(theme::BACKGROUND)
            .bg(theme::FOCUS)
            .add_modifier(Modifier::BOLD)
    } else if matches!(
        action,
        Action::ReviewPermission | Action::ReviewProviderRetry
    ) {
        Style::default()
            .fg(theme::ATTENTION)
            .bg(theme::PANEL)
            .add_modifier(Modifier::BOLD)
    } else if active_overview {
        Style::default()
            .fg(theme::FOCUS)
            .bg(theme::PANEL)
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
            Action::ResumeProvider => "[t] Continue",
            Action::ReviewPermission | Action::ReviewProviderRetry => "[a] Review",
            Action::InspectAttention => "Evidence",
            Action::FullDiagnostic => "Details",
            Action::OpenCandidate => "Open",
            Action::ClearSearch => "Clear",
            Action::RetryDiscovery => "Retry",
            Action::OpenEdit => "Diff",
            Action::KeepAll => "Keep *",
            Action::EditAll => "All",
            Action::ConfirmRevert => "Revert",
            Action::CancelRevert => "Cancel",
            Action::KeepFile => "Keep",
            Action::RevertFile | Action::RevertAll => "Revert",
            Action::ReviewOpen => "Evidence [e]",
            Action::ReviewBack => "Back",
            Action::Technical => "Technical",
            Action::Requests => "Requests",
            Action::OpenRequest => "Open",
            Action::UsageAccounting => "Accounting",
            Action::RecordedActivity => "Activity",
            Action::GiveInstructions => "Compose",
            Action::RecoverOwner => "Recover",
            Action::WriteGuidance => "Recovery",
            Action::Report => {
                if available >= 8 {
                    "[o] Read"
                } else {
                    "[o]"
                }
            }
            Action::ResultEvidence => "[e]",
            Action::Compose => "[i]",
            Action::More => {
                if available >= 8 {
                    "[.] More"
                } else {
                    "[.]"
                }
            }
            Action::Versions => "[v]",
            Action::Provenance => "[p]",
            Action::ReviewEdits => {
                if available >= 9 {
                    "[3] Edits"
                } else {
                    "[3]"
                }
            }
            Action::Open => {
                if available >= 12 {
                    "[Enter] Open"
                } else {
                    "[Enter]"
                }
            }
            Action::Close => "[Esc]",
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
                .border_type(if active_overview {
                    ratatui::widgets::BorderType::Double
                } else {
                    ratatui::widgets::BorderType::Rounded
                })
                .border_style(
                    Style::default().fg(if focused || primary || active_overview {
                        theme::FOCUS
                    } else {
                        theme::BORDER
                    }),
                ),
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
            assert!(text.contains("[i]"));
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
            tool_rerun: false,
            live: None,
            cancel_retry: None,
            session_id: None,
            prompts,
            answers,
            finished,
        });
        app.approval = Some(ApprovalPrompt {
            review: None,
            command: Some("ls".into()),
            content: "Scope: exact command".into(),
            mode: ApprovalMode::Human,
        });
        app.detail_scroll = 3;
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        let text = render(&mut app, 80, 24);
        assert!(text.contains("Approval pending"));
        for c in ['y', 'a', 's', 'p', 'g', 'n'] {
            key(&mut app, root.path(), &store, KeyCode::Char(c));
        }
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(app.approval.is_some());
        assert_eq!(app.detail_scroll, 3);
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        ft.send(Err(crate::tui_actions::RunnerError::Other(
            "fixture disconnected".into(),
        )))
        .unwrap();
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
            tool_rerun: false,
            live: None,
            cancel_retry: None,
            session_id: None,
            prompts,
            answers,
            finished,
        });
        app.approval = Some(ApprovalPrompt {
            review: None,
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
        let session = store.create("demo", "Workspace goal", "work").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
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
