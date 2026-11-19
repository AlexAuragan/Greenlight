//! Explicit message recipients, scoped memory drafts and creation presentation.
use super::*;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Hash)]
pub(super) enum Mode {
    #[default]
    Step,
    Manager,
    Command,
}
pub(super) type Scope = (Option<String>, Option<String>, Mode);
#[derive(Default)]
pub(super) struct Composer {
    pub mode: Mode,
    pub scope: Scope,
    pub explicit: bool,
    pub creation_goal: String,
    pub creation_workflow: String,
    pub last_instruction: Option<(Scope, String)>,
}
impl App {
    pub(super) fn can_resume_message(&self) -> bool {
        self.runner.is_none()
            && self
                .session()
                .is_some_and(|s| matches!(s.status.as_str(), "ready" | "blocked"))
            && !self.workspace.as_ref().is_some_and(|w| {
                matches!(
                    w.primary,
                    crate::workspace_view::PrimaryAction::RecoverOwner
                        | crate::workspace_view::PrimaryAction::RecoverWrites
                        | crate::workspace_view::PrimaryAction::Observe
                )
            })
    }
    pub(super) fn set_message_mode(&mut self, mode: Mode) {
        if self.compose.mode != mode {
            self.compose.mode = mode;
            self.sync_draft();
        }
        self.compose.explicit = true;
    }
    pub(super) fn choose_recipient(&mut self) {
        let mode = match self.compose.mode {
            Mode::Step => Mode::Manager,
            Mode::Manager | Mode::Command if self.can_resume_message() => Mode::Step,
            _ => Mode::Manager,
        };
        self.set_message_mode(mode);
        self.shell.actions_focus = false;
        self.focus = Focus::Compose;
    }
    pub(super) fn message_limit(&self) -> usize {
        if self.compose.mode == Mode::Manager {
            crate::manager::MESSAGE_BYTES
        } else {
            8192
        }
    }
    pub(super) fn send_message(&mut self, root: &Path, store: &Store) {
        self.sync_draft();
        if self.composer.trim().is_empty() {
            self.notice = "Enter a message first; nothing sent.".into();
            return;
        }
        if let Some(selected) = self.session() {
            match store.session(&selected.id) {
                Ok(current)
                    if current.active_branch == selected.active_branch
                        && (self.compose.mode != Mode::Step
                            || current.current_step == selected.current_step) => {}
                Ok(_) => {
                    self.notice="Target changed. Review the session/branch/step before sending; draft retained.".into();
                    return;
                }
                Err(e) => {
                    self.notice = format!("Target unavailable: {e:#}. Draft retained.");
                    return;
                }
            }
        }
        match self.compose.mode {
            Mode::Manager => self.start_manager(root, store, self.composer.clone()),
            Mode::Command => {
                if !self.composer.starts_with('/') {
                    self.notice =
                        "Commands begin with /. Choose a message recipient to send ordinary text."
                            .into();
                    return;
                }
                self.submit(root, store)
            }
            Mode::Step => {
                self.resume_instruction(root, store, self.composer.clone());
            }
        }
    }
    pub(super) fn resume_instruction(&mut self, root: &Path, store: &Store, instruction: String) {
        let result = (|| -> Result<Session> {
            if self.runner.is_some() {
                bail!("A workflow worker is already active; this draft was not sent.")
            }
            store.ensure_no_pending_writes()?;
            let selected = self.session().context("Choose a session first")?;
            let session = store.session(&selected.id)?;
            if !matches!(session.status.as_str(), "ready" | "blocked") {
                bail!("Session is {}; resume requires ready/blocked state. Recover or review a restart first.",session.status)
            }
            if session.active_branch != selected.active_branch
                || session.current_step != selected.current_step
            {
                bail!("Session branch/step changed; review the current target first")
            }
            let _lock = store.lock_idle_session(&session.id)?;
            Ok(session)
        })();
        let session = match result {
            Ok(s) => s,
            Err(e) => {
                self.notice = format!("Not sent: {e:#}. Draft retained.");
                return;
            }
        };
        self.compose.last_instruction = Some((self.compose.scope.clone(), instruction.clone()));
        self.runner = Some(tui_actions::resume(
            root.to_path_buf(),
            session.id,
            instruction,
        ));
        self.composer.clear();
        self.focus = self.main_focus();
        self.notice = "Starting step with your instructions. Approvals remain separate.".into();
    }
    pub(super) fn restore_instruction(&mut self) {
        if let Some((scope, text)) = &self.compose.last_instruction {
            if *scope != self.compose.scope {
                self.notice =
                    "Last instruction belongs to another session, branch or recipient.".into();
                return;
            }
            if !self.composer.is_empty() {
                self.notice =
                    "Current draft retained; clear it before restoring the previous instruction."
                        .into();
                return;
            }
            self.composer = text.clone();
            self.focus_composer();
            self.notice = "Previous instructions restored for review; nothing resent.".into();
        }
    }
    pub(super) fn remember_creation(&mut self) {
        if let Some(form) = &self.new_session {
            if let Some(goal) = &form.goal {
                self.compose.creation_goal = goal.clone();
            }
            if let Some(choice) = form.workflows.get(form.cursor) {
                self.compose.creation_workflow = choice.id.clone();
            }
        }
    }
    pub(super) fn new_form(&mut self, root: &Path) -> Result<()> {
        let mut form = crate::session_picker::NewSession::discover(root)?;
        if let Some(index) = form
            .workflows
            .iter()
            .position(|w| w.id == self.compose.creation_workflow)
        {
            form.cursor = index;
            if !self.compose.creation_goal.is_empty() {
                form.goal = Some(self.compose.creation_goal.clone());
            }
        }
        self.new_session = Some(form);
        self.shell.actions_focus = false;
        Ok(())
    }
}
pub(super) fn draw_composer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if area.height == 0 {
        return;
    }
    let title = match app.compose.mode {
        Mode::Step => "Resume step with instructions",
        Mode::Manager if app.manager_job.is_some() => "Manager answering · draft retained",
        Mode::Manager => "Ask manager",
        Mode::Command => "Commands",
    };
    let description = match app.compose.mode {
        Mode::Step if !app.can_resume_message() => {
            "Step delivery is unavailable; recover/restart or choose the manager."
        }
        Mode::Step => "Send & resume starts this step; tool approvals remain separate.",
        Mode::Manager => {
            "Read-only answer or route proposal; no workflow instruction is delivered."
        }
        Mode::Command => {
            "Explicit slash commands. Message modes treat slash-prefixed text literally."
        }
    };
    let block = Block::default()
        .padding(ratatui::widgets::Padding::horizontal(1))
        .style(Style::default().bg(theme::PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(if area.height >= 8 { 1 } else { 0 }),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(title).style(Style::default().add_modifier(Modifier::BOLD)),
        rows[0],
    );
    let recipient = match app.compose.mode {
        Mode::Step => "workflow step",
        Mode::Manager => "manager (read-only)",
        Mode::Command => "Greenlight controls",
    };
    let target = app
        .session()
        .map(|s| format!("To: {recipient} · {} / {}", s.workflow, s.current_step))
        .unwrap_or_else(|| "No selected session; choose one before sending.".into());
    frame.render_widget(
        Paragraph::new(super::view::fit(&target, rows[1].width as usize))
            .style(Style::default().fg(theme::MUTED)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(super::view::fit(description, rows[2].width as usize))
            .style(Style::default().fg(theme::MUTED)),
        rows[2],
    );
    app.composer_editor.draw(
        frame,
        &app.composer,
        "Type here…",
        rows[3],
        app.composer_active() && !app.shell.actions_focus,
    );
    frame.render_widget(
        Paragraph::new(super::view::fit(
            "Enter sends · Alt+Enter newline · Esc keeps draft",
            rows[4].width as usize,
        ))
        .style(Style::default().fg(theme::MUTED)),
        rows[4],
    );
}
pub(super) fn draw_creation(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(form) = &app.new_session else { return };
    let inner = ratatui::layout::Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    if let Some(goal) = &form.goal {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(2),
            Constraint::Length(2),
        ])
        .split(inner);
        frame.render_widget(
            Paragraph::new("New session · Describe the goal")
                .style(Style::default().add_modifier(Modifier::BOLD)),
            rows[0],
        );
        let title = form
            .workflows
            .get(form.cursor)
            .map(|w| w.title.as_str())
            .unwrap_or("Workflow unavailable");
        frame.render_widget(
            Paragraph::new(super::view::fit(
                &format!("Workflow: {title}"),
                rows[1].width as usize,
            ))
            .style(Style::default().fg(theme::MUTED)),
            rows[1],
        );
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::PANEL)),
            rows[2],
        );
        let edit = ratatui::layout::Rect::new(
            rows[2].x + 1,
            rows[2].y,
            rows[2].width.saturating_sub(2),
            rows[2].height,
        );
        app.goal_editor.draw(
            frame,
            goal,
            "What should this workflow achieve?",
            edit,
            !app.help && !app.shell.actions_focus,
        );
        frame.render_widget(Paragraph::new(form.error.as_deref().unwrap_or("Start creates a session and launches its first step.\nEnter starts · Alt+Enter newline · Back changes workflow")).style(Style::default().fg(if form.error.is_some(){theme::ERROR}else{theme::MUTED})).wrap(Wrap{trim:false}),rows[3]);
    } else {
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(inner);
        frame.render_widget(
            Paragraph::new("New session · Choose workflow")
                .style(Style::default().add_modifier(Modifier::BOLD)),
            rows[0],
        );
        let items = form.workflows.iter().map(|w| {
            ListItem::new(vec![
                Line::from(w.title.clone()),
                Line::styled(
                    if w.error.is_some() {
                        "Unavailable · inspect configuration".into()
                    } else {
                        w.summary.clone()
                    },
                    Style::default().fg(if w.error.is_some() {
                        theme::ERROR
                    } else {
                        theme::MUTED
                    }),
                ),
            ])
        });
        let mut state = ListState::default();
        if !form.workflows.is_empty() {
            state.select(Some(form.cursor));
        }
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("▸ ")
                .highlight_style(Style::default().bg(theme::PANEL)),
            rows[1],
            &mut state,
        );
        for index in state.offset()..form.workflows.len() {
            let y = rows[1].y + ((index - state.offset()) * 2) as u16;
            if y >= rows[1].bottom() {
                break;
            }
            app.shell.hit(
                ratatui::layout::Rect::new(
                    rows[1].x,
                    y,
                    rows[1].width,
                    2.min(rows[1].bottom() - y),
                ),
                shell::Action::Workflow(index),
            );
        }
        let hint = form
            .error
            .as_deref()
            .unwrap_or(if form.workflows.is_empty() {
                "No workflows found. Check this project's workflow configuration."
            } else {
                "Select a workflow, then Continue. Nothing starts while browsing."
            });
        frame.render_widget(
            Paragraph::new(safe_display(hint))
                .style(Style::default().fg(if form.error.is_some() {
                    theme::ERROR
                } else {
                    theme::MUTED
                }))
                .wrap(Wrap { trim: false }),
            rows[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
    };
    fn setup() -> (tempfile::TempDir, Store, Session, App) {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        let folder = root.path().join(".greenlight/workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Local fixture\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(
            folder.join("work.md"),
            "---\nid: work\nverification:\n  type: none\n---\nWork on the local fixture.\n",
        )
        .unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "Fixture goal", "work").unwrap();
        let mut app = App::new(Some(&session.id));
        app.refresh(&store).unwrap();
        (root, store, session, app)
    }
    fn server(root: &Path) -> (std::thread::JoinHandle<()>, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let env = format!("GREENLIGHT_COMPOSE_TEST_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&env, "local-fixture");
        fs::write(root.join(".greenlight/config.yaml"),format!("provider:\n  model: fixture\n  base_url: http://{address}/v1\n  api_key_env: {env}\nmanager:\n  enabled: true\n")).unwrap();
        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "No local request received");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 2048];
            let request: serde_json::Value = loop {
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                    }
                }
            };
            let input = request["messages"][1]["content"]
                .as_str()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            let content = if let Some(input) = input.filter(|v| v.get("message_id").is_some()) {
                serde_json::json!({"schema_version":1,"message_id":input["message_id"],"snapshot_id":input["evidence"]["snapshot_id"],"action":"answer","content":"Read-only fixture answer.","citations":["workflow:demo"]}).to_string()
            } else {
                "action: finish\nsummary: Local step complete".into()
            };
            let body=serde_json::json!({"choices":[{"message":{"content":content}}],"usage":{"prompt_tokens":12,"completion_tokens":8,"prompt_tokens_details":{"cached_tokens":0}}}).to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        (handle, env)
    }
    fn wait(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.runner.is_some() || app.manager_job.is_some() {
            app.tick();
            assert!(Instant::now() < deadline, "{}", app.notice);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn explicit_step_bypasses_manager_and_slash_prefixed_logs_are_verbatim() {
        let (root, store, session, mut app) = setup();
        let (http, env) = server(root.path());
        app.refresh(&store).unwrap();
        assert!(app.manager_enabled);
        app.set_message_mode(Mode::Step);
        app.focus_composer();
        let instruction = "  /recover\nThese are literal logs, not a command.  \n";
        app.composer = instruction.into();
        app.send_message(root.path(), &store);
        assert!(app.runner.is_some());
        assert!(app.manager_job.is_none());
        wait(&mut app);
        http.join().unwrap();
        std::env::remove_var(env);
        let session = store.session(&session.id).unwrap();
        assert_eq!(session.status, "completed");
        let history = store.history(&session).unwrap();
        assert!(history
            .iter()
            .any(|e| e.2 == "user.instruction" && e.3 == instruction));
        assert!(!history
            .iter()
            .any(|e| e.2 == "manager.message" || e.2 == "runner.recovered"));
    }
    #[test]
    fn manager_question_is_literal_and_does_not_resume_workflow() {
        let (root, store, session, mut app) = setup();
        let (http, env) = server(root.path());
        app.refresh(&store).unwrap();
        app.set_message_mode(Mode::Manager);
        app.focus_composer();
        app.composer = "/resume\nExplain these logs; don't execute them.".into();
        let original = app.composer.clone();
        app.send_message(root.path(), &store);
        assert!(app.manager_job.is_some());
        assert!(app.runner.is_none());
        wait(&mut app);
        http.join().unwrap();
        std::env::remove_var(env);
        assert_eq!(store.session(&session.id).unwrap().status, "ready");
        let history = store.history(&session).unwrap();
        assert!(history.iter().any(|e| e.2 == "manager.result"));
        assert!(history.iter().any(|e| e.2 == "manager.message"
            && serde_json::from_str::<serde_json::Value>(&e.3).unwrap()["text"] == original));
        assert!(!history.iter().any(|e| e.2 == "runner.started"));
    }
    #[test]
    fn drafts_are_separate_by_recipient_and_branch() {
        let (root, store, mut session, mut app) = setup();
        let original_branch = session.active_branch.clone();
        app.set_message_mode(Mode::Step);
        app.composer = "Step instructions".into();
        app.set_message_mode(Mode::Manager);
        assert!(app.composer.is_empty());
        app.composer = "Manager question".into();
        app.set_message_mode(Mode::Step);
        assert_eq!(app.composer, "Step instructions");
        let event = store.history(&session).unwrap()[0].0;
        store.fork(&mut session, event, "work").unwrap();
        app.refresh(&store).unwrap();
        assert!(app.composer.is_empty());
        app.composer = "Other branch".into();
        store
            .switch_branch(&mut session, &original_branch, "work")
            .unwrap();
        app.refresh(&store).unwrap();
        assert_eq!(app.composer, "Step instructions");
        app.set_message_mode(Mode::Manager);
        assert_eq!(app.composer, "Manager question");
        assert!(store
            .history(&session)
            .unwrap()
            .iter()
            .all(|e| e.2 != "manager.message"));
        drop(root);
    }
    #[test]
    fn failed_preflight_preserves_draft_and_stale_target_cannot_receive_it() {
        let (root, store, mut session, mut app) = setup();
        app.set_message_mode(Mode::Step);
        app.composer = "Keep my instructions".into();
        let event = store.history(&session).unwrap()[0].0;
        store.fork(&mut session, event, "work").unwrap();
        app.send_message(root.path(), &store);
        assert!(app.notice.contains("Target changed"));
        assert_eq!(app.composer, "Keep my instructions");
        assert!(app.runner.is_none());
        app.refresh(&store).unwrap();
        app.set_message_mode(Mode::Step);
        app.composer = "Never overwrite".into();
        let owner = store.claim_execution(&session.id, None).unwrap();
        app.send_message(root.path(), &store);
        assert!(app.runner.is_none());
        assert_eq!(app.composer, "Never overwrite");
        drop(owner);
    }
    #[test]
    fn editor_tab_reaches_recipient_and_help_without_submitting() {
        let (root, store, _session, mut app) = setup();
        app.set_message_mode(Mode::Step);
        app.focus_composer();
        app.composer = "Unsent log".into();
        app.on_key(
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.composer, "Unsent logh");
        app.on_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert!(app.shell.actions_focus);
        assert!(app.runner.is_none());
        let actions = app.shell_actions();
        let help = actions
            .iter()
            .position(|a| *a == shell::Action::Help)
            .unwrap();
        for _ in 0..help {
            app.on_key(
                KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                root.path(),
                &store,
            )
            .unwrap();
        }
        app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert!(app.help);
        app.on_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        assert_eq!(app.focus, Focus::Compose);
        assert_eq!(app.composer, "Unsent logh");
        app.activate(shell::Action::Recipient, root.path(), &store)
            .unwrap();
        assert_eq!(app.compose.mode, Mode::Manager);
        assert!(app.composer.is_empty());
        assert!(app.runner.is_none());
        assert!(app.manager_job.is_none());
    }
    #[test]
    fn creation_back_cancel_and_reopen_keep_goal_without_creating_session() {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(None);
        app.new_form(root.path()).unwrap();
        app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            root.path(),
            &store,
        )
        .unwrap();
        app.new_session.as_mut().unwrap().goal = Some("Retained creation goal".into());
        app.activate(shell::Action::BackWorkflow, root.path(), &store)
            .unwrap();
        assert!(app.new_session.as_ref().unwrap().goal.is_none());
        app.activate(shell::Action::Continue, root.path(), &store)
            .unwrap();
        assert_eq!(
            app.new_session.as_ref().unwrap().goal.as_deref(),
            Some("Retained creation goal")
        );
        app.activate(shell::Action::Cancel, root.path(), &store)
            .unwrap();
        assert!(app.new_session.is_none());
        assert!(store.list().unwrap().is_empty());
        app.new_form(root.path()).unwrap();
        assert_eq!(
            app.new_session.as_ref().unwrap().goal.as_deref(),
            Some("Retained creation goal")
        );
        app.new_session.as_mut().unwrap().goal = Some("  ".into());
        app.activate(shell::Action::StartWorkflow, root.path(), &store)
            .unwrap();
        assert!(app.new_session.as_ref().unwrap().error.is_some());
        assert!(store.list().unwrap().is_empty());
        app.activate(shell::Action::ClearGoal, root.path(), &store)
            .unwrap();
        app.activate(shell::Action::Cancel, root.path(), &store)
            .unwrap();
        app.new_form(root.path()).unwrap();
        assert!(app.new_session.as_ref().unwrap().goal.is_none());
    }
}
