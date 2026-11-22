//! Pane focus, selection, drafts and scroll state.
use super::*;

impl App {
    pub(super) fn switch_view(&mut self, view: View) {
        if self.view == view {
            return;
        }
        self.remember_reading();
        self.attention_return = false;
        self.view = view;
        if view != View::Conversation {
            self.result_open = false;
        }
        if self.focus != Focus::Compose && self.focus != Focus::Detail {
            self.focus = match view {
                View::Conversation => Focus::Events,
                View::Artifacts | View::Edits => Focus::Artifacts,
            };
        }
        self.restore_reading();
    }
    pub(super) fn main_focus(&self) -> Focus {
        match self.view {
            View::Conversation => Focus::Events,
            View::Artifacts | View::Edits => Focus::Artifacts,
        }
    }
    pub(super) fn focus_composer(&mut self) {
        if self.focus != Focus::Compose {
            self.focus_before_compose = self.focus;
        }
        if !self.compose.explicit && self.composer.is_empty() {
            let mode = if self.manager_enabled {
                compose::Mode::Conversation
            } else if !self.can_resume_message() {
                compose::Mode::Manager
            } else {
                compose::Mode::Step
            };
            self.compose.mode = mode;
            self.sync_draft();
        }
        self.focus = Focus::Compose;
    }
    pub(super) fn focus_left(&mut self) {
        match self.focus {
            Focus::Sessions => self.close_navigator(),
            Focus::Detail => {
                self.inspector_open = false;
                if self.attention_return {
                    self.blocker_card = self.attention.is_some();
                    self.attention_return = false;
                }
                self.focus = self.main_focus();
            }
            Focus::Events | Focus::Artifacts => {
                if self.shell.sessions {
                    self.close_navigator();
                } else {
                    self.open_navigator(false);
                }
            }
            _ => {}
        }
    }
    pub(super) fn scroll_detail(&mut self, delta: i16) {
        let current = self
            .rendered_scroll
            .get()
            .filter(|(requested, _)| *requested == self.detail_scroll)
            .map(|(_, clamped)| clamped)
            .unwrap_or(self.detail_scroll);
        self.detail_scroll = current.saturating_add_signed(delta);
    }
    pub(super) fn sync_draft(&mut self) {
        let selected = self.session().map(|s| s.id.clone());
        let branch = self.session().map(|s| s.active_branch.clone());
        let scope = (selected.clone(), branch, self.compose.mode);
        if scope != self.compose.scope {
            if scope.0 != self.compose.scope.0 || scope.1 != self.compose.scope.1 {
                self.compose.explicit = false;
            }
            self.drafts.insert(
                self.compose.scope.clone(),
                std::mem::take(&mut self.composer),
            );
            self.composer = self.drafts.remove(&scope).unwrap_or_default();
            self.compose.scope = scope;
            self.composer_editor.reset();
        }
        self.composer_session = selected;
    }
    pub(super) fn open_help(&mut self) {
        self.shell.help_scroll = self.detail_scroll;
        self.shell.help_actions = false;
        self.shell.help_action_index = 0;
        self.help = true;
        self.detail_scroll = 0;
    }
    pub(super) fn too_small(&self) -> bool {
        self.viewport.0 < 40
            || self.viewport.1 < 14
            || ((self.approval_visible() || self.edits.pending.is_some()) && self.viewport.1 < 20)
    }
    pub(super) fn navigate(&mut self, delta: isize) {
        match self.focus {
            Focus::Compose | Focus::Detail => (),
            Focus::Sessions => self.navigation.select(delta),
            Focus::Events => {
                self.follow_activity = false;
                let next = move_index(self.event_index, delta, self.events.len());
                if next != self.event_index {
                    self.event_index = next;
                    self.detail_scroll = 0;
                    self.diff = None;
                }
            }
            Focus::Artifacts if self.view == View::Edits => {
                self.edits.select(delta);
                self.detail_scroll = 0;
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
    pub(super) fn select_end(&mut self, last: bool) {
        match self.focus {
            Focus::Compose | Focus::Detail => (),
            Focus::Sessions => self
                .navigation
                .select(if last { isize::MAX } else { isize::MIN }),
            Focus::Events => {
                self.follow_activity = last;
                self.event_index = if last {
                    self.events.len().saturating_sub(1)
                } else {
                    0
                };
                self.detail_scroll = 0;
                self.diff = None;
            }
            Focus::Artifacts if self.view == View::Edits => {
                self.edits.index = if last {
                    self.edits.files.len().saturating_sub(1)
                } else {
                    0
                };
                self.edits.select(0);
                self.detail_scroll = 0;
            }
            Focus::Artifacts => {
                self.artifact_index = if last {
                    self.artifacts.len().saturating_sub(1)
                } else {
                    0
                };
                self.detail_scroll = 0;
            }
        }
    }
}
