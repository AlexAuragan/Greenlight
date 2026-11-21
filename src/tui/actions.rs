//! Typed UI commands and their shared shortcut/label definitions.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    RetrySaved,
    RetrySavedManager,
    RerunTool,
    InspectAttention,
    ReviewPermission,
    ReviewProviderRetry,
    ResumeProvider,
    FullDiagnostic,
    ConversationDump,
    RefreshDump,
    SaveDump,
    CopyDump,
    RecordedActivity,
    GiveInstructions,
    AskManager,
    RecoverOwner,
    WriteGuidance,
    RestartReview,
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
    Overview,
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
    ReturnLive,
    Copy,
    Editor,
    Filter,
    Usage,
    Technical,
    Provider,
    Requests,
    OpenRequest,
    UsageAccounting,
    DetailsBack,
    UsageRequest(usize),
    OpenCandidate,
    CloseNavigator,
    ClearSearch,
    NavSearch,
    RetryDiscovery,
    OpenEdit,
    KeepFile,
    KeepAll,
    RevertFile,
    RevertAll,
    ConfirmRevert,
    CancelRevert,
    EditAll,
    ReviewOpen,
    ReviewBack,
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
        if let Some(binding) = BINDINGS.iter().find(|binding| binding.action == self) {
            return binding.label;
        }
        match self {
            Self::InspectAttention => "Inspect evidence",
            Self::ConversationDump => "Conversation dump",
            Self::FullDiagnostic => "Full diagnostic",
            Self::RecordedActivity => "Recorded activity",
            Self::GiveInstructions => "Give instructions",
            Self::AskManager => "Ask manager",
            Self::RecoverOwner => "Recover owner",
            Self::WriteGuidance => "Recovery guidance",
            Self::RestartReview => "Restart review",
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
            Self::Compare => "Compare previous",
            Self::OpenCandidate => "Open selected",
            Self::CloseNavigator => "Back",
            Self::ClearSearch => "Clear search",
            Self::RetryDiscovery => "Retry discovery",
            Self::OpenEdit => "Open selected diff",
            Self::KeepFile => "Keep file",
            Self::KeepAll => "Keep all",
            Self::RevertFile => "Preview file revert",
            Self::RevertAll => "Preview all reverts",
            Self::EditAll => "All files",
            Self::ReviewBack => "Back to review",
            Self::Usage => "Usage",
            Self::Technical => "Technical details",
            Self::Provider => "Provider / API key",
            Self::Requests => "Request details",
            Self::OpenRequest => "Open request",
            Self::UsageAccounting => "Full accounting",
            Self::DetailsBack => "Back",
            Self::UsageRequest(_) => "Request",
            Self::Follow => "Follow latest",
            _ => "Select",
        }
    }
}

pub(super) struct Binding {
    pub action: Action,
    pub key: KeyCode,
    pub label: &'static str,
}

macro_rules! binding {
    ($action:ident, $key:literal, $name:literal) => {
        Binding {
            action: Action::$action,
            key: KeyCode::Char($key),
            label: concat!("[", $key, "] ", $name),
        }
    };
}

pub(super) const BINDINGS: &[Binding] = &[
    binding!(ResumeProvider, 't', "Continue step"),
    binding!(ReviewProviderRetry, 'a', "Review retry"),
    binding!(ReviewPermission, 'a', "Review permission"),
    binding!(RefreshDump, 'R', "Refresh"),
    binding!(CopyDump, 'C', "Copy full dump"),
    binding!(SaveDump, 'w', "Save full dump"),
    binding!(Overview, 's', "Overview"),
    binding!(Report, 'o', "Read final report"),
    binding!(ResultEvidence, 'e', "Full result"),
    binding!(ReviewEdits, '3', "Review edits"),
    binding!(Versions, 'v', "Versions"),
    binding!(Provenance, 'p', "Version details"),
    binding!(Sessions, 'f', "Sessions"),
    binding!(Compose, 'i', "Compose"),
    binding!(Details, 'c', "Details"),
    binding!(Help, 'h', "Help"),
    binding!(More, '.', "More"),
    binding!(Activity, '1', "Activity"),
    binding!(Artifacts, '2', "Artifacts"),
    binding!(Edits, '3', "Edits"),
    binding!(Copy, 'C', "Copy"),
    binding!(Editor, 'E', "Editor"),
    binding!(Filter, 'F', "Filter"),
    binding!(New, 'n', "New session"),
    binding!(Preview, 'P', "Toggle preview"),
    binding!(Expand, 'Z', "Expand preview"),
    binding!(RetrySaved, 't', "Retry saved request"),
    binding!(RetrySavedManager, 'T', "Retry saved manager"),
    binding!(RerunTool, 'R', "Run tool again"),
    binding!(Stop, 'k', "Stop"),
    binding!(Mouse, 'M', "Toggle mouse"),
    binding!(ReviewOpen, 'e', "Full evidence"),
    binding!(NavSearch, 'f', "Search"),
    binding!(ConfirmRevert, 'y', "Revert"),
    binding!(CancelRevert, 'n', "Cancel"),
    Binding {
        action: Action::ReturnLive,
        key: KeyCode::Enter,
        label: "[Enter] Live output",
    },
    Binding {
        action: Action::Open,
        key: KeyCode::Enter,
        label: "[Enter] Open selected",
    },
    Binding {
        action: Action::Close,
        key: KeyCode::Esc,
        label: "[Esc] Back",
    },
];

impl Action {
    pub(super) fn matches_key(self, key: KeyCode) -> bool {
        BINDINGS
            .iter()
            .any(|binding| binding.action == self && binding.key == key)
            || self == Self::Help && key == KeyCode::Char('?')
    }
    pub(super) fn for_key(key: KeyCode, available: &[Self]) -> Option<Self> {
        available
            .iter()
            .copied()
            .find(|action| action.matches_key(key))
    }
}

impl App {
    pub(super) fn submit_new_session(&mut self, root: &Path, store: &Store) -> Result<()> {
        if self.new_session.is_none() {
            return Ok(());
        }
        let form = self.new_session.as_mut().unwrap();
        if form.goal.is_none() {
            if let Some(choice) = form.workflows.get(form.cursor) {
                if let Some(error) = &choice.error {
                    form.error = Some(error.clone());
                } else {
                    form.goal = Some(self.compose.creation_goal.clone());
                    self.goal_editor.reset();
                    form.error = None;
                }
            }
        } else {
            match form.create(root, store) {
                Ok(session) => {
                    self.new_session = None;
                    self.session_query.clear();
                    self.requested_session = Some(session.id.clone());
                    self.diagnostic = None;
                    self.context_detail = None;
                    self.details.close();
                    self.refresh(store)?;
                    self.focus = Focus::Events;
                    self.runner = Some(tui_actions::resume(
                        root.to_path_buf(),
                        session.id,
                        String::new(),
                    ));
                    self.view = View::Conversation;
                    self.follow_activity = true;
                    self.activity_query.clear();
                    self.compose.creation_goal.clear();
                    self.compose.creation_workflow.clear();
                    self.notice =
                        "Starting workflow. Model output and approval requests will appear here."
                            .into();
                }
                Err(error) => form.error = Some(format!("Cannot create session: {error:#}")),
            }
        }
        Ok(())
    }

    pub(super) fn open_selection(&mut self, root: &Path, store: &Store) -> Result<()> {
        if self.permission_popup_visible() {
            return Ok(());
        }
        if self.human_review().is_some() {
            self.checkpoint_open(store);
        } else if self.new_session.is_some() {
            self.submit_new_session(root, store)?;
        } else if self.searching {
            self.searching = false;
            self.refresh(store)?;
        } else if self.details.page == Some(details::Page::Requests) {
            if self.details.selected_call().is_some() {
                self.open_run_details(details::Page::Request, store);
            }
        } else if let Some(browser) = &mut self.artifact_browser {
            browser.reading = !browser.reading;
            self.detail_scroll = 0;
        } else if self.result_open && self.view == View::Conversation {
            self.result_open = false;
            self.focus = Focus::Events;
            self.detail_scroll = 0;
        } else if self.view == View::Edits {
            self.shell.actions_focus = true;
            self.shell.action_index = 0;
        } else if self.blocker_card
            && self.view == View::Conversation
            && self.focus == Focus::Events
        {
            self.inspect_blocker();
        } else if self.focus == Focus::Detail {
            self.focus_left();
        } else if self.focus == Focus::Sessions {
            self.open_candidate(store)?;
        } else {
            self.inspector_open = true;
            self.focus = Focus::Detail;
            self.follow_activity = false;
        }
        Ok(())
    }

    pub(super) fn close_surface(&mut self, store: &Store) -> Result<()> {
        if self.help {
            self.help = false;
            self.refresh_checkpoint(store);
            self.detail_scroll = self.shell.help_scroll;
        } else if self.permission_popup_visible() {
            self.dismiss_permission();
        } else if self.human_review().is_some() && self.checkpoint.reading {
            self.checkpoint_back(store);
        } else if self.approval_visible() {
            self.deliver_checked_approval('n', store)?;
        } else if self.shell.more {
            self.shell.more = false;
        } else if self.new_session.is_some() {
            self.remember_creation();
            self.new_session = None;
            self.notice = "New session cancelled.".into();
        } else if self.searching {
            self.searching = false;
            self.session_query.clear();
            self.refresh(store)?;
        } else if self.activity_filtering {
            self.activity_query.clear();
            self.commit_activity_filter();
            self.activity_filtering = false;
            self.filter_activity();
        } else if self.edits.pending.is_some() {
            self.edits.pending = None;
            self.detail_scroll = 0;
            self.notice = "Revert cancelled; no files changed.".into();
        } else if self.artifact_browser.is_some() {
            self.artifact_browser = None;
            self.detail_scroll = 0;
        } else if self.diagnostic.is_some() || self.context_detail.is_some() {
            self.clear_diagnostic();
            self.details.close();
            self.context_detail = None;
            self.detail_scroll = 0;
        } else if self.details.page.is_some() {
            self.back_run_details(store);
        } else if self.restart_plan.is_some() || !self.restart_choices.is_empty() {
            self.restart_plan = None;
            self.restart_choices.clear();
            self.notice = "Restart cancelled; no files modified.".into();
        } else if self.focus == Focus::Compose {
            self.focus = self.focus_before_compose;
        } else if self.result_open && self.view == View::Conversation {
            self.result_open = false;
            self.focus = Focus::Events;
            self.detail_scroll = 0;
        } else {
            self.focus_left();
        }
        Ok(())
    }
}

/// Input ownership for decisions; Help never forwards a key to the layer below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DecisionLayer {
    Help,
    Permission,
    HumanEvidence,
    HumanChoice,
    Revert,
    Workspace,
}

impl App {
    pub(super) fn decision_layer(&self) -> DecisionLayer {
        if self.help {
            DecisionLayer::Help
        } else if self.permission_popup_visible() {
            DecisionLayer::Permission
        } else if self.approval_visible() {
            if self.human_review().is_some() && self.checkpoint.reading {
                DecisionLayer::HumanEvidence
            } else {
                DecisionLayer::HumanChoice
            }
        } else if self.edits.pending.is_some() {
            DecisionLayer::Revert
        } else {
            DecisionLayer::Workspace
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_keys_and_direct_commands_have_the_same_effects() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "work").unwrap();
        for action in [
            Action::Activity,
            Action::Artifacts,
            Action::Edits,
            Action::Details,
            Action::Compose,
            Action::Preview,
            Action::Expand,
            Action::New,
            Action::Sessions,
            Action::Filter,
        ] {
            let binding = BINDINGS
                .iter()
                .find(|binding| binding.action == action)
                .unwrap();
            let mut keyboard = App::new(Some(&session.id));
            let mut button = App::new(Some(&session.id));
            keyboard.refresh(&store).unwrap();
            button.refresh(&store).unwrap();
            keyboard
                .on_key(
                    KeyEvent::new(binding.key, KeyModifiers::NONE),
                    root.path(),
                    &store,
                )
                .unwrap();
            button.activate(action, root.path(), &store).unwrap();
            assert_eq!(
                (keyboard.view, keyboard.focus, keyboard.details.page),
                (button.view, button.focus, button.details.page),
                "{action:?}"
            );
            assert_eq!(
                (
                    keyboard.shell.preview,
                    keyboard.shell.expanded,
                    keyboard.new_session.is_some()
                ),
                (
                    button.shell.preview,
                    button.shell.expanded,
                    button.new_session.is_some()
                ),
                "{action:?}"
            );
            assert_eq!(keyboard.diagnostic, button.diagnostic, "{action:?}");
        }
    }

    #[test]
    fn help_owns_commands_above_a_pending_decision() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        app.approval = Some(ApprovalPrompt {
            review: None,
            content: "fixture".into(),
            mode: ApprovalMode::Tool,
            command: None,
        });
        assert_eq!(app.decision_layer(), DecisionLayer::Permission);
        app.activate(Action::Help, root.path(), &store).unwrap();
        assert!(!app.help);
        app.dismiss_permission();
        app.activate(Action::Help, root.path(), &store).unwrap();
        assert_eq!(app.decision_layer(), DecisionLayer::Help);
        app.activate(Action::Decision('y'), root.path(), &store)
            .unwrap();
        app.activate(Action::Compose, root.path(), &store).unwrap();
        assert!(app.approval.is_some());
        assert_ne!(app.focus, Focus::Compose);
        app.activate(Action::Close, root.path(), &store).unwrap();
        assert_eq!(app.decision_layer(), DecisionLayer::Workspace);
        assert!(app.approval_hidden);
        app.reopen_permission();
        assert_eq!(app.decision_layer(), DecisionLayer::Permission);
    }
    #[test]
    fn closing_filter_by_button_restores_the_same_rows_as_escape() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "work").unwrap();
        let mut keyboard = App::new(Some(&session.id));
        let mut button = App::new(Some(&session.id));
        for app in [&mut keyboard, &mut button] {
            app.refresh(&store).unwrap();
            app.activity_query = "no matching event".into();
            app.activity_filtering = true;
            app.filter_activity();
            assert!(app.events.is_empty());
        }
        keyboard
            .on_key(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                root.path(),
                &store,
            )
            .unwrap();
        button.activate(Action::Close, root.path(), &store).unwrap();
        assert!(!button.activity_filtering);
        assert!(button.activity_query.is_empty());
        assert!(!button.events.is_empty());
        assert_eq!(button.events.len(), keyboard.events.len());
    }
}
