//! Keyboard precedence and modal input routing.
use super::*;

impl App {
    pub(super) fn on_key(&mut self, key: KeyEvent, root: &Path, store: &Store) -> Result<bool> {
        self.remember_reading();
        let overlay = self.reading_overlay();
        self.reading.modal = overlay;
        let result = self.route_key(key, root, store);
        self.reading.modal = false;
        if overlay && !self.reading_overlay() {
            self.restore_reading();
        }
        self.remember_reading();
        result
    }
    fn route_key(&mut self, key: KeyEvent, root: &Path, store: &Store) -> Result<bool> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(false);
        }
        // One Enter press performs one navigation/submission action.
        if key.code == KeyCode::Enter && key.kind == KeyEventKind::Repeat {
            return Ok(false);
        }

        self.sync_draft();
        if self.approval.is_none()
            && self.focus != Focus::Compose
            && !self.searching
            && self.new_session.is_none()
            && key.code == KeyCode::Char('x')
        {
            if let Some(cancel) = self.runner.as_ref().and_then(|r| r.cancel_retry.as_ref()) {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                self.notice="Stop requested: retry waits cancel promptly; an in-flight request may finish, and no further provider request will start.".into();
                return Ok(false);
            }
            if self.manager_job.is_some() {
                self.cancel_manager();
                return Ok(false);
            }
        }
        if self.too_small() {
            if key.code == KeyCode::Esc && self.approval.is_some() {
                self.deliver_approval('n')?;
            }
            return Ok(key.code == KeyCode::Char('q')
                && self.runner.is_none()
                && self.manager_job.is_none());
        }
        if let Some(mode) = self.approval.as_ref().map(|request| request.mode) {
            if key.kind != KeyEventKind::Press {
                return Ok(false);
            }
            match key.code {
                KeyCode::Char(choice)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && mode.accepts(choice.to_ascii_lowercase()) =>
                {
                    self.deliver_approval(choice.to_ascii_lowercase())?;
                }
                KeyCode::Esc => self.deliver_approval('n')?,
                // Enter is intentionally inert. Approval must be an explicit
                // choice, never an accidental confirmation of a default.
                KeyCode::Enter => {}
                KeyCode::PageDown => self.scroll_detail(8),
                KeyCode::PageUp => self.scroll_detail(-8),
                KeyCode::Home => self.detail_scroll = 0,
                KeyCode::End => self.detail_scroll = u16::MAX,
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                _ => {}
            }
            return Ok(false);
        }

        if self.activity_filtering {
            match key.code {
                KeyCode::Esc => {
                    self.activity_query.clear();
                    self.commit_activity_filter();
                    self.activity_filtering = false;
                }
                KeyCode::Enter => {
                    self.commit_activity_filter();
                    self.activity_filtering = false;
                }
                _ => self
                    .filter_editor
                    .key(&mut self.activity_query, key, false, 256),
            }
            self.filter_activity();
            return Ok(false);
        }
        if key.kind == KeyEventKind::Press
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && (self.focus != Focus::Compose || self.reading_overlay())
            && !self.searching
            && self.new_session.is_none()
        {
            match key.code {
                KeyCode::Char('C') => {
                    self.start_copy();
                    return Ok(false);
                }
                KeyCode::Char('E') => {
                    self.request_editor();
                    return Ok(false);
                }
                _ => {}
            }
        }
        if self.edits.pending.is_some() {
            match key.code {
                KeyCode::Char('y')
                    if key.kind == KeyEventKind::Press
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
                {
                    self.confirm_revert(root, store)
                }
                KeyCode::Char('n') | KeyCode::Esc if key.kind == KeyEventKind::Press => {
                    self.edits.pending = None;
                    self.detail_scroll = 0;
                    self.notice = "Revert cancelled; no files changed.".into();
                }
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::PageUp => self.scroll_detail(-8),
                KeyCode::PageDown => self.scroll_detail(8),
                KeyCode::Home => self.detail_scroll = 0,
                KeyCode::End => self.detail_scroll = u16::MAX,
                _ => {}
            }
            return Ok(false);
        }
        if self.artifact_browser.is_some() {
            if key.kind != KeyEventKind::Press
                && matches!(
                    key.code,
                    KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('p') | KeyCode::Char('m')
                )
            {
                return Ok(false);
            }
            let mut browser = self.artifact_browser.take().unwrap();
            let session = self.session().cloned().context("No session selected")?;
            match key.code {
                KeyCode::Esc => {
                    self.detail_scroll = 0;
                    return Ok(false);
                }
                KeyCode::Left => {
                    browser.reading = false;
                    self.detail_scroll = 0;
                }
                KeyCode::Enter if browser.reading => {
                    browser.reading = false;
                    self.detail_scroll = 0;
                }
                KeyCode::Enter | KeyCode::Right => {
                    browser.reading = true;
                    self.detail_scroll = 0;
                }
                KeyCode::Char('d') => {
                    browser.mode = crate::artifact_history::Mode::Diff;
                    browser.load(store, &session);
                    browser.reading = true;
                    self.detail_scroll = 0;
                }
                KeyCode::Char('p') => {
                    browser.mode = crate::artifact_history::Mode::Provenance;
                    browser.load(store, &session);
                    browser.reading = true;
                    self.detail_scroll = 0;
                }
                KeyCode::Char('m') => {
                    browser.mode = crate::artifact_history::Mode::Content;
                    self.render_markdown = !self.render_markdown;
                    browser.load(store, &session);
                    self.detail_scroll = 0;
                }
                KeyCode::Char('o') => {
                    browser.mode = crate::artifact_history::Mode::Content;
                    browser.load(store, &session);
                    browser.reading = true;
                    self.detail_scroll = 0;
                }
                KeyCode::Down | KeyCode::Char('j') if browser.reading => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') if browser.reading => self.scroll_detail(-1),
                KeyCode::PageDown if browser.reading => self.scroll_detail(8),
                KeyCode::PageUp if browser.reading => self.scroll_detail(-8),
                KeyCode::Home if browser.reading => self.detail_scroll = 0,
                KeyCode::End if browser.reading => self.detail_scroll = u16::MAX,
                KeyCode::Down | KeyCode::Char('j') => {
                    browser.index = move_index(browser.index, 1, browser.versions.len());
                    browser.load(store, &session);
                    self.detail_scroll = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    browser.index = move_index(browser.index, -1, browser.versions.len());
                    browser.load(store, &session);
                    self.detail_scroll = 0;
                }
                _ => {}
            }
            self.artifact_browser = Some(browser);
            return Ok(false);
        }
        if self.help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => {
                    self.help = false;
                    self.detail_scroll = 0;
                }
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                KeyCode::PageDown => self.scroll_detail(8),
                KeyCode::PageUp => self.scroll_detail(-8),
                KeyCode::Home => self.detail_scroll = 0,
                KeyCode::End => self.detail_scroll = u16::MAX,
                _ => {}
            }
            return Ok(false);
        }
        if self.result_open
            && self.view == View::Conversation
            && self.focus != Focus::Compose
            && !self.searching
            && self.new_session.is_none()
            && self.diagnostic.is_none()
            && self.context_detail.is_none()
            && self.usage_detail.is_none()
            && self.restart_plan.is_none()
            && self.restart_choices.is_empty()
        {
            match key.code {
                KeyCode::Enter if self.focus != Focus::Sessions => {
                    self.result_open = false;
                    self.focus = Focus::Events;
                    self.detail_scroll = 0;
                    return Ok(false);
                }
                KeyCode::Esc | KeyCode::Char('h') => {
                    self.result_open = false;
                    self.focus = Focus::Events;
                    self.detail_scroll = 0;
                    return Ok(false);
                }
                KeyCode::Up | KeyCode::Char('k') if self.focus != Focus::Sessions => {
                    self.scroll_detail(-1);
                    return Ok(false);
                }
                KeyCode::Down | KeyCode::Char('j') if self.focus != Focus::Sessions => {
                    self.scroll_detail(1);
                    return Ok(false);
                }
                KeyCode::PageUp if self.focus != Focus::Sessions => {
                    self.scroll_detail(-8);
                    return Ok(false);
                }
                KeyCode::PageDown if self.focus != Focus::Sessions => {
                    self.scroll_detail(8);
                    return Ok(false);
                }
                KeyCode::Home if self.focus != Focus::Sessions => {
                    self.detail_scroll = 0;
                    return Ok(false);
                }
                KeyCode::End if self.focus != Focus::Sessions => {
                    self.detail_scroll = u16::MAX;
                    return Ok(false);
                }
                _ => {}
            }
        }
        let scrolling = self.approval.is_some()
            || self.diagnostic.is_some()
            || self.usage_detail.is_some()
            || self.context_detail.is_some()
            || self.restart_plan.is_some()
            || self.focus == Focus::Detail
            || (self.blocker_card && self.focus == Focus::Events);
        if scrolling
            && self.new_session.is_none()
            && !self.searching
            && self.focus != Focus::Compose
        {
            match key.code {
                KeyCode::PageDown => {
                    self.scroll_detail(8);
                    return Ok(false);
                }
                KeyCode::PageUp => {
                    self.scroll_detail(-8);
                    return Ok(false);
                }
                KeyCode::Home => {
                    self.detail_scroll = 0;
                    return Ok(false);
                }
                KeyCode::End => {
                    self.detail_scroll = u16::MAX;
                    return Ok(false);
                }
                _ => {}
            }
        }
        if self.new_session.is_some() {
            if key.kind != KeyEventKind::Press
                && !matches!(
                    key.code,
                    KeyCode::Char(_)
                        | KeyCode::Backspace
                        | KeyCode::Delete
                        | KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Up
                        | KeyCode::Down
                )
            {
                return Ok(false);
            }
            match key.code {
                KeyCode::Esc => {
                    self.new_session = None;
                    self.notice = "New session cancelled.".into();
                }
                KeyCode::Enter
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                {
                    let form = self.new_session.as_mut().unwrap();
                    if form.goal.is_none() {
                        if let Some(choice) = form.workflows.get(form.cursor) {
                            if let Some(error) = &choice.error {
                                form.error = Some(error.clone());
                            } else {
                                form.goal = Some(String::new());
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
                                self.usage_detail = None;
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
                                self.notice = "Starting workflow. Model output and approval requests will appear here.".into();
                            }
                            Err(error) => {
                                form.error = Some(format!("Cannot create session: {error:#}"))
                            }
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j')
                    if self.new_session.as_ref().unwrap().goal.is_none() =>
                {
                    let form = self.new_session.as_mut().unwrap();
                    form.cursor = move_index(form.cursor, 1, form.workflows.len());
                    form.error = None;
                }
                KeyCode::Up | KeyCode::Char('k')
                    if self.new_session.as_ref().unwrap().goal.is_none() =>
                {
                    let form = self.new_session.as_mut().unwrap();
                    form.cursor = move_index(form.cursor, -1, form.workflows.len());
                    form.error = None;
                }
                _ => {
                    if let Some(goal) = self.new_session.as_mut().unwrap().goal.as_mut() {
                        self.goal_editor.key(goal, key, true, 8192);
                    }
                }
            }
            return Ok(false);
        }
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.session_query.clear();
                }
                KeyCode::Enter => {
                    self.searching = false;
                }
                KeyCode::Down => self.navigate(1),
                KeyCode::Up => self.navigate(-1),
                _ => self
                    .session_editor
                    .key(&mut self.session_query, key, false, 256),
            }
            self.refresh(store)?;
            return Ok(false);
        }

        if self.diagnostic.is_some() || self.usage_detail.is_some() || self.context_detail.is_some()
        {
            match key.code {
                KeyCode::Esc => {
                    self.diagnostic = None;
                    self.usage_detail = None;
                    self.context_detail = None;
                    self.detail_scroll = 0;
                }
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                KeyCode::Char('q') if self.runner.is_none() && self.manager_job.is_none() => {
                    return Ok(true)
                }
                _ => {}
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
                    self.restart_cursor =
                        move_index(self.restart_cursor, 1, self.restart_choices.len());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.restart_cursor =
                        move_index(self.restart_cursor, -1, self.restart_choices.len());
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
                KeyCode::Enter
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                {
                    self.submit(root, store)
                }
                _ => self
                    .composer_editor
                    .key(&mut self.composer, key, true, 8192),
            }
            return Ok(false);
        }

        match key.code {
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('F') if self.view == View::Conversation => {
                self.activity_filtering = true;
                self.filter_editor.reset();
            }
            KeyCode::Char('f') => {
                self.searching = true;
                self.session_editor.reset();
                self.focus = Focus::Sessions;
            }
            KeyCode::Char('n') => {
                if self.runner.is_some() {
                    self.notice =
                        "Wait for the active worker before creating another session.".into();
                } else {
                    match crate::session_picker::NewSession::discover(root) {
                        Ok(form) => self.new_session = Some(form),
                        Err(error) => {
                            self.raise_diagnostic(format!("Cannot list workflows: {error:#}"))
                        }
                    }
                }
            }
            KeyCode::Char('c') => {
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
                self.detail_scroll = 0;
            }
            KeyCode::Char('o') if self.view == View::Conversation => {
                self.open_card_artifact(store)?
            }
            KeyCode::Char('e') if self.workspace.as_ref().is_some_and(|w| w.blocker.is_some()) => {
                self.inspect_blocker()
            }
            KeyCode::Char('h') => {
                self.result_open = false;
                self.blocker_card = false;
                self.view = View::Conversation;
                self.focus = Focus::Events;
            }
            KeyCode::Char('b') => {
                self.blocker_card = true;
                self.view = View::Conversation;
                self.focus = Focus::Events;
            }
            KeyCode::Char('r') => self.open_restart(root, store),
            KeyCode::Char('q') if self.runner.is_none() && self.manager_job.is_none() => {
                return Ok(true)
            }
            KeyCode::Char('q') => self.notice = if self.manager_job.is_some() {
                "Manager request active; wait for its result. /cancel-manager stops future retries."
            } else {
                "Agent running; wait for its result before exiting."
            }
            .into(),
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
            KeyCode::Char('3') => {
                self.switch_view(View::Edits);
                self.refresh_edits(store);
            }
            KeyCode::Char('s') if self.session().is_some_and(|s| s.status == "completed") => {
                self.switch_view(View::Conversation);
                self.result_open = true;
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
            }
            KeyCode::Char('u') if self.view == View::Edits => {
                self.prepare_revert(root, store, false)
            }
            KeyCode::Char('U') if self.view == View::Edits => {
                self.prepare_revert(root, store, true)
            }
            KeyCode::Char('m') if self.view == View::Artifacts => {
                self.render_markdown = !self.render_markdown;
                self.detail_scroll = 0;
            }
            KeyCode::Enter => {
                if self.blocker_card
                    && self.view == View::Conversation
                    && self.focus == Focus::Events
                {
                    self.inspect_blocker();
                } else if self.focus == Focus::Detail {
                    self.focus_left();
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
                if self.focus == Focus::Detail
                    || (self.blocker_card
                        && self.view == View::Conversation
                        && self.focus == Focus::Events)
                {
                    self.scroll_detail(1);
                } else {
                    self.navigate(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus == Focus::Detail
                    || (self.blocker_card
                        && self.view == View::Conversation
                        && self.focus == Focus::Events)
                {
                    self.scroll_detail(-1);
                } else {
                    self.navigate(-1);
                }
            }
            KeyCode::Char('g') | KeyCode::Home => {
                if self.focus == Focus::Detail {
                    self.detail_scroll = 0;
                } else {
                    self.select_end(false);
                }
            }
            KeyCode::Char('G') | KeyCode::End => {
                if self.focus != Focus::Detail {
                    self.select_end(true);
                }
            }
            KeyCode::Char('v') if self.view == View::Artifacts => {
                let name = self.artifact().map(|a| a.name.clone()).or_else(|| {
                    self.events
                        .iter()
                        .rev()
                        .find(|e| e.kind == "artifact.created")
                        .map(|e| e.data.clone())
                });
                if let (Some(session), Some(name)) = (self.session(), name) {
                    match crate::artifact_history::Browser::open(store, session, &name) {
                        Ok(browser) => {
                            self.artifact_browser = Some(browser);
                            self.detail_scroll = 0;
                        }
                        Err(error) => {
                            self.raise_diagnostic(format!("Version history unavailable: {error:#}"))
                        }
                    }
                } else {
                    self.notice = "No artifact selected.".into();
                }
            }
            KeyCode::Char('v') if self.view == View::Conversation => self.toggle_verbose(),
            KeyCode::Char('d') if self.view == View::Conversation => self.toggle_diff(store)?,
            _ => {}
        }
        Ok(false)
    }
}
