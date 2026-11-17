//! Pane focus, selection, drafts and scroll state.
use super::*;

impl App {
    pub(super) fn switch_view(&mut self, view: View) {
        if self.view == view {
            return;
        }
        self.remember_reading();
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
        self.focus = Focus::Compose;
    }
    pub(super) fn focus_left(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => { self.shell.sessions=false; self.main_focus() },
            Focus::Detail => { self.inspector_open = false; self.main_focus() },
            Focus::Events | Focus::Artifacts => { self.shell.sessions = !self.shell.sessions; if self.shell.sessions {Focus::Sessions} else {self.main_focus()} },
            other => other,
        };
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
        if selected != self.composer_session {
            self.drafts.insert(
                self.composer_session.take(),
                std::mem::take(&mut self.composer),
            );
            self.composer = self.drafts.remove(&selected).unwrap_or_default();
            self.composer_session = selected;
            self.composer_editor.reset();
        }
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
            || (self.approval.is_some() && self.viewport.1 < 20)
    }
    pub(super) fn navigate(&mut self, delta: isize) {
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
            Focus::Sessions => {
                let next = if last {
                    self.sessions.len().saturating_sub(1)
                } else {
                    0
                };
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
