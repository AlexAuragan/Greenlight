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
        // Help owns input above every decision; no keys fall through to a prompt.
        if self.help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('h' | '?') => {
                    self.help = false;
                    self.refresh_checkpoint(store);
                    self.detail_scroll = self.shell.help_scroll;
                }
                KeyCode::Char('M') => self.shell.mouse = !self.shell.mouse,
                KeyCode::Char('C') => self.start_copy(),
                KeyCode::Tab | KeyCode::BackTab => {
                    if self.shell.help_actions {
                        self.shell.help_action_index = 1 - self.shell.help_action_index;
                    } else {
                        self.shell.help_actions = true;
                        self.shell.help_action_index =
                            if key.code == KeyCode::BackTab { 1 } else { 0 };
                    }
                }
                KeyCode::Left | KeyCode::Right if self.shell.help_actions => {
                    self.shell.help_action_index = 1 - self.shell.help_action_index
                }
                KeyCode::Enter if self.shell.help_actions => {
                    if self.shell.help_action_index == 0 {
                        self.help = false;
                        self.detail_scroll = self.shell.help_scroll;
                    } else {
                        self.shell.mouse = !self.shell.mouse;
                    }
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
        let typing = self.composer_active()
            || self.searching
            || self.activity_filtering
            || self.new_session.as_ref().is_some_and(|f| f.goal.is_some());
        if (self.searching
            || self.activity_filtering
            || self.new_session.is_some()
            || self.composer_active())
            && (matches!(key.code, KeyCode::Tab | KeyCode::BackTab) || self.shell.actions_focus)
        {
            let actions = self.shell_actions();
            match key.code {
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                    let reverse = matches!(key.code, KeyCode::BackTab | KeyCode::Left);
                    if self.shell.actions_focus {
                        self.shell.action_index = (self.shell.action_index
                            + if reverse { actions.len() - 1 } else { 1 })
                            % actions.len();
                    } else {
                        self.shell.actions_focus = true;
                        self.shell.action_index = if reverse { actions.len() - 1 } else { 0 };
                    }
                }
                KeyCode::Esc => self.shell.actions_focus = false,
                KeyCode::Enter => {
                    if let Some(action) = actions.get(self.shell.action_index).copied() {
                        self.shell.actions_focus = false;
                        return self.activate(action, root, store);
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        if !typing && !self.shell.more {
            let shortcut=match key.code {
                KeyCode::Char('.')=>Some(shell::Action::More),
                KeyCode::Char('i')=>Some(shell::Action::Compose),
                KeyCode::Char('s')=>Some(shell::Action::Overview),
                KeyCode::Char('o')=>Some(shell::Action::Report),
                KeyCode::Char('e')=>Some(shell::Action::ResultEvidence),
                KeyCode::Char('3')=>Some(shell::Action::ReviewEdits),
                KeyCode::Char('v')=>Some(shell::Action::Versions),
                KeyCode::Char('p')=>Some(shell::Action::Provenance),
                _=>None,
            };
            if let Some(action)=shortcut.filter(|a|self.footer_actions().contains(a)){return self.activate(action,root,store);}
        }
        if !typing && matches!(key.code, KeyCode::Char('h' | '?')) {
            self.open_help();
            return Ok(false);
        }
        if self.shell.more {
            let actions = self.more_actions();
            match key.code {
                KeyCode::Esc => self.shell.more = false,
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    self.shell.menu_index = (self.shell.menu_index + 1) % actions.len()
                }
                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                    self.shell.menu_index =
                        (self.shell.menu_index + actions.len() - 1) % actions.len()
                }
                KeyCode::Enter => {
                    let action = actions[self.shell.menu_index.min(actions.len() - 1)];
                    self.shell.more = false;
                    return self.activate(action, root, store);
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.shell.sessions && self.approval.is_none() && self.new_session.is_none() {
            let actions=self.shell_actions();
            match key.code {
                KeyCode::Esc if self.shell.actions_focus => self.shell.actions_focus=false,
                KeyCode::Esc => self.close_navigator(),
                KeyCode::Tab | KeyCode::BackTab => {
                    if self.shell.actions_focus {self.shell.action_index=(self.shell.action_index+if key.code==KeyCode::BackTab{actions.len()-1}else{1})%actions.len();}
                    else{self.shell.actions_focus=true;self.shell.action_index=if key.code==KeyCode::BackTab{actions.len()-1}else{0};}
                }
                KeyCode::Enter if self.shell.actions_focus => {
                    if let Some(action)=actions.get(self.shell.action_index).copied(){return self.activate(action,root,store);}
                }
                KeyCode::Left | KeyCode::Right if self.shell.actions_focus => {
                    self.shell.action_index=(self.shell.action_index+if key.code==KeyCode::Left{actions.len()-1}else{1})%actions.len();
                }
                _ if self.shell.actions_focus => {}
                KeyCode::Char('q') if !self.searching => {
                    if self.runner.is_none() && self.manager_job.is_none(){return Ok(true)}
                    self.notice="Worker or manager is active; stop or wait before exiting.".into();
                }
                KeyCode::Enter => self.open_candidate(store)?,
                KeyCode::PageDown => self.navigation.select(5),
                KeyCode::PageUp => self.navigation.select(-5),
                KeyCode::Left if !self.searching => self.close_navigator(),
                KeyCode::Char('i') if !self.searching => {self.close_navigator();self.focus_composer();}
                KeyCode::Down => self.navigation.select(1),
                KeyCode::Up => self.navigation.select(-1),
                KeyCode::Char('j') if !self.searching => self.navigation.select(1),
                KeyCode::Char('k') if !self.searching => self.navigation.select(-1),
                KeyCode::Home if !self.searching => self.navigation.select(isize::MIN),
                KeyCode::End if !self.searching => self.navigation.select(isize::MAX),
                KeyCode::Char('f') if !self.searching => {self.searching=true;self.session_editor.reset();}
                KeyCode::Char('n') if !self.searching => {self.close_navigator();return self.on_key(key,root,store);}
                _ if self.searching => {self.session_editor.key(&mut self.session_query,key,false,256);self.refresh(store)?;}
                _ => {}
            }
            return Ok(false);
        }
        if !typing && self.view==View::Edits && !self.blocking_surface() && !self.shell.actions_focus && self.focus!=Focus::Sessions {
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    self.edits.cycle(key.code==KeyCode::BackTab);
                    self.focus=if self.edits.overview{Focus::Artifacts}else{Focus::Detail};
                    self.detail_scroll=0;
                    return Ok(false);
                }
                KeyCode::Enter => {self.shell.actions_focus=true;self.shell.action_index=0;return Ok(false);}
                _ => {}
            }
        }
        if !typing
            && (!self.blocking_surface()
                || (self.navigation.error.is_some() || self.navigation.load_error.is_some()) && self.approval.is_none()
                || self.details.page.is_some() && self.approval.is_none() && self.diagnostic.is_none()
                || self.diagnostic.is_some()
                    && self.approval.is_none()
                    && self.context_detail.is_none()
                || self.artifact_browser.is_some()
                    && self.approval.is_none()
                    && self.diagnostic.is_none()
                    && self.context_detail.is_none())
        {
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                let actions = self.shell_actions();
                if !actions.is_empty() {
                    if !self.shell.actions_focus {
                        self.shell.action_index = if key.code == KeyCode::BackTab {
                            actions.len() - 1
                        } else {
                            0
                        };
                    } else {
                        self.shell.action_index = (self.shell.action_index
                            + if key.code == KeyCode::BackTab {
                                actions.len() - 1
                            } else {
                                1
                            })
                            % actions.len();
                    }
                    self.shell.actions_focus = true;
                }
                return Ok(false);
            }
            if self.shell.actions_focus {
                match key.code {
                    KeyCode::Esc => self.shell.actions_focus = false,
                    KeyCode::Left => {
                        self.shell.action_index = self.shell.action_index.saturating_sub(1)
                    }
                    KeyCode::Right => {
                        self.shell.action_index =
                            (self.shell.action_index + 1) % self.shell_actions().len()
                    }
                    KeyCode::Enter => {
                        if let Some(action) =
                            self.shell_actions().get(self.shell.action_index).copied()
                        {
                            self.shell.actions_focus = false;
                            return self.activate(action, root, store);
                        }
                    }
                    _ => {}
                }
                return Ok(false);
            }
        }

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
        if self.human_review().is_some() && self.checkpoint.reading {
            match key.code {
                KeyCode::Esc => self.checkpoint_back(store),
                KeyCode::Char('C') => self.start_copy(),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                KeyCode::PageDown => self.scroll_detail(8),
                KeyCode::PageUp => self.scroll_detail(-8),
                KeyCode::Home => self.detail_scroll=0,
                KeyCode::End => self.detail_scroll=u16::MAX,
                _ => {}
            }
            return Ok(false);
        }
        if self.too_small() {
            if key.code == KeyCode::Esc && self.approval.is_some() {
                self.deliver_checked_approval('n', store)?;
            }
            if key.code == KeyCode::Esc && self.edits.pending.is_some() {self.edits.pending=None;self.notice="Revert cancelled; no files changed.".into();}
            return Ok(key.code == KeyCode::Char('q')
                && self.runner.is_none()
                && self.manager_job.is_none());
        }
        if let Some(mode) = self.approval.as_ref().map(|request| request.mode) {
            if key.kind != KeyEventKind::Press {
                return Ok(false);
            }
            if self.human_review().is_some() {
                match key.code {
                    KeyCode::Tab | KeyCode::Right => {self.checkpoint_select(1);return Ok(false);}
                    KeyCode::BackTab | KeyCode::Left => {self.checkpoint_select(-1);return Ok(false);}
                    KeyCode::Enter | KeyCode::Char('e') => {self.checkpoint_open(store);return Ok(false);}
                    _ => {}
                }
            }
            match key.code {
                KeyCode::Char(choice)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && mode.accepts(choice.to_ascii_lowercase()) =>
                {
                    self.deliver_checked_approval(choice.to_ascii_lowercase(), store)?;
                }
                KeyCode::Esc => self.deliver_checked_approval('n', store)?,
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
        if self.artifact_browser.is_some() && key.code==KeyCode::Char('s') && self.session().is_some_and(|s|s.status=="completed") {
            return self.activate(shell::Action::Overview,root,store);
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
        if self.details.page.is_some() && self.approval.is_none() && self.diagnostic.is_none() {
            match key.code {
                KeyCode::Esc => self.back_run_details(store),
                KeyCode::Char('C') => self.start_copy(),
                KeyCode::Char('E') => self.request_editor(),
                KeyCode::Down | KeyCode::Char('j') if self.details.page == Some(details::Page::Requests) => self.move_usage_request(1, store),
                KeyCode::Up | KeyCode::Char('k') if self.details.page == Some(details::Page::Requests) => self.move_usage_request(-1, store),
                KeyCode::PageDown if self.details.page == Some(details::Page::Requests) => self.move_usage_request(8, store),
                KeyCode::PageUp if self.details.page == Some(details::Page::Requests) => self.move_usage_request(-8, store),
                KeyCode::Home if self.details.page == Some(details::Page::Requests) => self.move_usage_request(isize::MIN, store),
                KeyCode::End if self.details.page == Some(details::Page::Requests) => self.move_usage_request(isize::MAX, store),
                KeyCode::Enter if self.details.page == Some(details::Page::Requests) => {
                    if self.details.selected_call().is_some() { self.open_run_details(details::Page::Request, store); }
                }
                KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
                KeyCode::PageDown => self.scroll_detail(8),
                KeyCode::PageUp => self.scroll_detail(-8),
                KeyCode::Home => self.detail_scroll = 0,
                KeyCode::End => self.detail_scroll = u16::MAX,
                KeyCode::Char('q') if self.runner.is_none() && self.manager_job.is_none() => return Ok(true),
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
            && self.restart_plan.is_none()
            && self.restart_choices.is_empty()
        {
            match key.code {
                KeyCode::Char('o') if self.result_overview.as_ref().is_some_and(|r|r.report.is_some()) => return self.activate(shell::Action::Report,root,store),
                KeyCode::Char('e') if self.result_overview.as_ref().is_none_or(|r|r.report.is_none()) => return self.activate(shell::Action::ResultEvidence,root,store),
                KeyCode::Enter if self.focus != Focus::Sessions => {
                    self.result_open = false;
                    self.focus = Focus::Events;
                    self.detail_scroll = 0;
                    return Ok(false);
                }
                KeyCode::Esc | KeyCode::Char('H') => {
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
                    self.remember_creation();
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

        if self.diagnostic.is_some() || self.context_detail.is_some()
        {
            match key.code {
                KeyCode::Esc => {
                    self.clear_diagnostic();
                    self.details.close();
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
                KeyCode::Tab => {
                    self.focus = self.focus_before_compose;
                    self.shell.actions_focus = true;
                    self.shell.action_index = 0;
                }
                KeyCode::Esc => {
                    self.focus = self.focus_before_compose;
                }
                KeyCode::Enter
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                {
                    self.send_message(root, store)
                }
                _ => {
                    let limit = self.message_limit();
                    self.composer_editor
                        .key(&mut self.composer, key, true, limit)
                }
            }
            return Ok(false);
        }

        match key.code {
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('P') => self.shell.preview = !self.shell.preview,
            KeyCode::Char('Z') => self.shell.expanded = !self.shell.expanded,
            KeyCode::Char('F') if self.view == View::Conversation => {
                self.activity_filtering = true;
                self.filter_editor.reset();
            }
            KeyCode::Char('f') => self.open_navigator(true),
            KeyCode::Char('n') => {
                if self.runner.is_some() {
                    self.notice =
                        "Wait for the active worker before creating another session.".into();
                } else {
                    match self.new_form(root) {
                        Ok(()) => {}
                        Err(error) => {
                            self.raise_diagnostic(format!("Cannot list workflows: {error:#}"))
                        }
                    }
                }
            }
            KeyCode::Char('c') => self.open_run_details(details::Page::Run, store),
            KeyCode::Char('o') if self.view == View::Conversation => {
                self.open_card_artifact(store)?
            }
            KeyCode::Char('e') if self.workspace.as_ref().is_some_and(|w| w.blocker.is_some()) => {
                self.inspect_blocker()
            }
            KeyCode::Char('H') => {
                self.attention_return = false;
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
                self.shell.actions_focus = true;
                self.shell.action_index = 0;
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
                } else if self.focus == Focus::Sessions {
                    self.open_candidate(store)?;
                } else {
                    self.inspector_open = true;
                    self.focus = Focus::Detail;
                    self.follow_activity = false;
                }
            }
            KeyCode::Char('i') => self.focus_composer(),
            KeyCode::Char('/') => {
                self.set_message_mode(compose::Mode::Command);
                self.focus_composer();
                if self.composer.is_empty() {
                    self.composer.push('/');
                }
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
