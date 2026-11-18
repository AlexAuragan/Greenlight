//! In-memory reading positions keyed by session/branch, independent of overlays.
use super::*;
#[derive(Clone)]
struct ActivityPosition {
    event: Option<i64>,
    scroll: u16,
    detail: bool,
    follow: bool,
    query: String,
    blocker: Option<bool>,
    diff: Option<(i64, std::sync::Arc<str>)>,
}
impl Default for ActivityPosition {
    fn default() -> Self {
        Self {
            event: None,
            scroll: 0,
            detail: false,
            follow: true,
            query: String::new(),
            blocker: None,
            diff: None,
        }
    }
}
#[derive(Clone, Default)]
struct ArtifactPosition {
    name: Option<String>,
    version: Option<i64>,
    scroll: u16,
    detail: bool,
}
#[derive(Clone, Default)]
struct Position {
    activity: ActivityPosition,
    artifacts: ArtifactPosition,
}
#[derive(Default)]
pub(super) struct Reading {
    scope: Option<(String, String)>,
    saved: HashMap<(String, String), Position>,
    pub modal: bool,
}
impl App {
    pub(super) fn reading_overlay(&self) -> bool {
        self.shell.sessions || self.details.page.is_some() || self.shell.more || self.approval.is_some()
            || self.edits.pending.is_some()
            || (self.result_open && self.view == View::Conversation)
            || self.diagnostic.is_some()
            || self.context_detail.is_some()
            || self.help
            || self.searching
            || self.activity_filtering
            || self.new_session.is_some()
            || self.artifact_browser.is_some()
            || self.restart_plan.is_some()
            || !self.restart_choices.is_empty()
    }
    pub(super) fn remember_reading(&mut self) {
        if self.reading.modal || self.reading_overlay() {
            return;
        }
        let Some(scope) = self.reading.scope.clone() else {
            return;
        };
        if !self
            .session()
            .is_some_and(|s| s.id == scope.0 && s.active_branch == scope.1)
        {
            return;
        }
        let focus = if self.focus == Focus::Compose {
            self.focus_before_compose
        } else {
            self.focus
        };
        let event = self.event().map(|e| e.id);
        let artifact = self.artifact().map(|a| (a.name.clone(), a.event_id));
        let previous_diff = self
            .reading
            .saved
            .get(&scope)
            .and_then(|p| p.activity.diff.as_ref());
        let diff = self
            .diff
            .as_ref()
            .filter(|(id, _)| Some(*id) == event)
            .map(|(id, text)| {
                (
                    *id,
                    previous_diff
                        .filter(|(previous, _)| previous == id)
                        .map(|(_, text)| text.clone())
                        .unwrap_or_else(|| std::sync::Arc::from(text.as_str())),
                )
            });
        let saved = self.reading.saved.entry(scope).or_default();
        match self.view {
            View::Conversation => {
                saved.activity = ActivityPosition {
                    event,
                    scroll: self.detail_scroll,
                    detail: focus == Focus::Detail || self.inspector_open,
                    follow: self.follow_activity,
                    query: self.activity_query.clone(),
                    blocker: Some(self.blocker_card),
                    diff,
                }
            }
            View::Edits => {}
            View::Artifacts => {
                saved.artifacts = ArtifactPosition {
                    name: artifact.as_ref().map(|a| a.0.clone()),
                    version: artifact.map(|a| a.1),
                    scroll: self.detail_scroll,
                    detail: focus == Focus::Detail || self.inspector_open,
                }
            }
        }
    }
    pub(super) fn reading_scope(&mut self) -> bool {
        let scope = self
            .session()
            .map(|s| (s.id.clone(), s.active_branch.clone()));
        if scope == self.reading.scope {
            return false;
        }
        self.reading.scope = scope;
        self.activity_query = self
            .reading
            .scope
            .as_ref()
            .and_then(|scope| self.reading.saved.get(scope))
            .map(|p| p.activity.query.clone())
            .unwrap_or_default();
        true
    }
    pub(super) fn restore_reading(&mut self) {
        if self.reading_overlay() {
            return;
        }
        let saved = self
            .reading
            .scope
            .as_ref()
            .and_then(|scope| self.reading.saved.get(scope))
            .cloned()
            .unwrap_or_default();
        let event = saved
            .activity
            .event
            .and_then(|id| self.events.iter().position(|e| e.id == id));
        self.follow_activity = saved.activity.follow;
        if let Some(blocker) = saved.activity.blocker {
            self.blocker_card = blocker;
        }
        self.event_index = if saved.activity.follow {
            self.events.len().saturating_sub(1)
        } else {
            event.unwrap_or(self.events.len().saturating_sub(1))
        };
        self.diff = saved
            .activity
            .diff
            .as_ref()
            .filter(|(id, _)| self.event().is_some_and(|event| event.id == *id))
            .map(|(id, text)| (*id, text.to_string()));
        self.artifact_index = saved
            .artifacts
            .name
            .as_ref()
            .and_then(|name| self.artifacts.iter().position(|a| &a.name == name))
            .unwrap_or(0);
        let (detail, scroll) = match self.view {
            View::Conversation => (
                saved.activity.detail,
                if self.event().map(|e| e.id) == saved.activity.event {
                    saved.activity.scroll
                } else {
                    0
                },
            ),
            View::Edits => (false, 0),
            View::Artifacts => (
                saved.artifacts.detail,
                if self.artifact().map(|a| a.event_id) == saved.artifacts.version {
                    saved.artifacts.scroll
                } else {
                    0
                },
            ),
        };
        self.inspector_open = detail;
        self.detail_scroll = scroll;
        self.rendered_scroll.set(None);
        let focus = if detail {
            Focus::Detail
        } else {
            self.main_focus()
        };
        if self.focus == Focus::Compose {
            self.focus_before_compose = focus;
        } else if self.focus != Focus::Sessions {
            self.focus = focus;
        }
    }
    pub(super) fn commit_activity_filter(&mut self) {
        if let Some(scope) = self.reading.scope.clone() {
            let saved = self.reading.saved.entry(scope).or_default();
            saved.activity.query = self.activity_query.clone();
            saved.activity.follow = false;
        }
    }
    pub(super) fn filter_activity(&mut self) {
        let selected = self.event().map(|e| e.id);
        self.events = self
            .activity_rows
            .iter()
            .filter(|event| self.activity_matches(event))
            .cloned()
            .collect();
        self.event_index = selected
            .and_then(|id| self.events.iter().position(|e| e.id == id))
            .unwrap_or(self.events.len().saturating_sub(1));
        if self.event().map(|e| e.id) != selected {
            self.detail_scroll = 0;
            self.diff = None;
        }
    }
    pub(super) fn activity_matches(&self, event: &EventRow) -> bool {
        if self.activity_query.is_empty() {
            return true;
        }
        let card = self.activity_cards.cards.get(&event.id);
        let text = format!(
            "{} {} {} {}",
            event.step,
            event.kind,
            card.map(|c| c.title.as_str()).unwrap_or(""),
            card.map(|c| c.summary.as_str()).unwrap_or("")
        );
        text.to_lowercase()
            .contains(&self.activity_query.to_lowercase())
    }
}
