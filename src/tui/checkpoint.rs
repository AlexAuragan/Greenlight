//! Human decisions have typed provenance; document readers never accept decisions.
use super::*;
use crate::tools::{HumanReview, HumanReviewKind};
#[derive(Default)]
pub(super) struct State {
    pub index: usize,
    pub reading: bool,
    pub error: Option<String>,
    pub text: String,
}
impl App {
    pub(super) fn human_review(&self) -> Option<&HumanReview> {
        self.approval
            .as_ref()
            .filter(|p| p.mode == ApprovalMode::Human)?
            .review
            .as_ref()
    }
    fn checkpoint_valid(&self, store: &Store) -> Result<()> {
        let Some(r) = self.human_review() else {
            return Ok(());
        };
        if self.runner.as_ref().and_then(|w| w.session_id.as_deref()) != Some(r.session.as_str()) {
            bail!("Review owner unavailable. This viewer cannot approve another worker's request.");
        }
        let s = store.session(&r.session)?;
        if s.active_branch != r.branch || s.current_step != r.step || s.status != "running" {
            bail!("Session, branch or step changed. This review cannot approve the changed run.");
        }
        if store.execution_state(&s.id)? != crate::session_execution::ExecutionState::Live {
            bail!("Execution owner is no longer live. The approval is unavailable.");
        }
        if store.active_attempt(&s, &r.step)?.0 != r.attempt {
            bail!("The attempt changed. Review the current attempt before approving.");
        }
        if r.kind == HumanReviewKind::Checkpoint {
            let id = r.manifest.context("Pinned-input manifest not recorded")?;
            let raw = store
                .history(&s)?
                .into_iter()
                .find(|e| e.0 == id && e.1 == r.step && e.2 == "context.inputs")
                .context("Pinned-input manifest unavailable")?
                .3;
            let manifest: crate::input_pins::InputManifest = serde_json::from_str(&raw)?;
            if manifest.attempt != r.attempt
                || manifest.artifacts.len() != r.documents.len()
                || manifest.artifacts.iter().any(|v| {
                    !r.documents
                        .iter()
                        .any(|d| d.name == v.name && d.event_id == v.event_id)
                })
            {
                bail!("The recorded input manifest does not match this review.");
            }
        }
        for doc in &r.documents {
            let exact = store.artifact_version(&s, &doc.name, doc.event_id)?;
            if exact.content != doc.content {
                bail!("Recorded document body changed: {}", doc.name);
            }
            if !store
                .artifacts(&s, &doc.step)?
                .iter()
                .any(|a| a.name == doc.name && a.event_id == doc.event_id)
            {
                bail!("A displayed document has a newer version: {}. Decline and review the new inputs.",doc.name);
            }
        }
        Ok(())
    }
    pub(super) fn refresh_checkpoint(&mut self, store: &Store) {
        self.checkpoint.error = self.checkpoint_valid(store).err().map(|e| format!("{e:#}"));
    }
    pub(super) fn checkpoint_open(&mut self, store: &Store) {
        if self.human_review().is_some() {
            self.refresh_checkpoint(store);
            self.checkpoint.text = self.review_document_text().unwrap_or_default();
            self.checkpoint.reading = true;
            self.detail_scroll = 0;
            self.shell.actions_focus = false;
        }
    }
    pub(super) fn checkpoint_back(&mut self, store: &Store) {
        self.checkpoint.reading = false;
        self.detail_scroll = 0;
        self.refresh_checkpoint(store);
    }
    pub(super) fn checkpoint_select(&mut self, delta: isize) {
        let len = self.human_review().map(|r| r.documents.len()).unwrap_or(0);
        self.checkpoint.index = move_index(self.checkpoint.index, delta, len);
        self.detail_scroll = 0;
    }
    pub(super) fn deliver_checked_approval(&mut self, choice: char, store: &Store) -> Result<()> {
        if self.human_review().is_some() {
            if self.checkpoint.reading {
                return Ok(());
            }
            self.refresh_checkpoint(store);
            if choice == 'y' && self.checkpoint.error.is_some() {
                self.notice =
                    "Approval unavailable: review the changed state; Decline remains available."
                        .into();
                return Ok(());
            }
        }
        self.deliver_approval(choice)
    }
    pub(super) fn review_document_text(&self) -> Option<String> {
        let r = self.human_review()?;
        let mut text = format!(
            "{}\nSession: {}\nBranch: {}\nStep: {}\nAttempt: {}\n\nReview question\n{}\n\n",
            title(r.kind),
            r.session,
            r.branch,
            r.step,
            r.attempt,
            r.question
        );
        if let Some(error) = &self.checkpoint.error {
            text.push_str(&format!("Review unavailable\n{error}\n\n"));
        }
        if let Some(d) = r.documents.get(self.checkpoint.index) {
            text.push_str(&format!(
                "Selected document: {} · event #{}\nProduced by: {}\n\n{}",
                d.name, d.event_id, d.step, d.content
            ));
        } else {
            text.push_str("No document body recorded for this review.");
        }
        text.push_str(if r.kind==HumanReviewKind::Checkpoint {"\n\nCheckpoint approval applies to the complete pinned input selection, not just this document. The engine rechecks the complete selection before recording approval."} else {"\n\nThese are available result artifacts, not a pinned-input approval contract. The backend records verification.human for this decision."});
        Some(text)
    }
}
fn title(kind: HumanReviewKind) -> &'static str {
    match kind {
        HumanReviewKind::Checkpoint => "Human checkpoint",
        HumanReviewKind::StepResult => "Review step result",
    }
}
pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    review: &HumanReview,
    area: ratatui::layout::Rect,
) {
    if app.checkpoint.reading {
        reader::draw_document(
            frame,
            app,
            reader::Document {
                tag: format!(
                    "checkpoint:{}:{}",
                    app.shell.request_epoch, app.checkpoint.index
                ),
                source: &app.checkpoint.text,
                markdown: false,
                headers: vec![Line::styled(
                    "Read-only evidence · Esc returns · decisions disabled",
                    Style::default().fg(theme::FOCUS),
                )],
            },
            Block::default(),
            area,
        );
        return;
    }
    let panels = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(2),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                title(review.kind),
                Style::default()
                    .fg(theme::ATTENTION)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                format!(
                    "Step: {} · local workflow review",
                    inline_display(&review.step)
                ),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        panels[0],
    );
    let compact = area.width < 60 || area.height < 16;
    let mut lines = vec![Line::styled(
        "Review question · excerpt",
        Style::default().add_modifier(Modifier::BOLD),
    )];
    lines.push(Line::raw(super::view::fit(
        &review.question,
        area.width.saturating_sub(1) as usize * if compact { 1 } else { 2 },
    )));
    if !compact {
        lines.push(Line::default());
        lines.push(Line::styled(
            if review.kind == HumanReviewKind::Checkpoint {
                "Pinned input documents"
            } else {
                "Available result evidence (not pinned approval inputs)"
            },
            Style::default().add_modifier(Modifier::BOLD),
        ));
    }
    if review.documents.is_empty() {
        lines.push(Line::raw("No document bodies recorded."));
    } else {
        let d = &review.documents[app.checkpoint.index.min(review.documents.len() - 1)];
        lines.push(Line::styled(
            super::view::fit(
                &format!(
                    "{} / {} · {}",
                    app.checkpoint.index + 1,
                    review.documents.len(),
                    inline_display(&d.name)
                ),
                area.width as usize,
            ),
            Style::default().fg(theme::FOCUS),
        ));
        if !compact {
            let excerpt = d.content.chars().take(160).collect::<String>();
            lines.push(Line::raw(format!(
                "Excerpt only: {}",
                inline_display(&excerpt)
            )));
        }
    }
    lines.push(Line::styled(
        if compact {
            "Tab cycles · e opens full evidence"
        } else {
            "Tab/Shift+Tab cycles documents · Enter/e opens exact evidence"
        },
        Style::default().fg(theme::MUTED),
    ));
    draw_scrolled(
        frame,
        app,
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        panels[1],
    );
    let status=app.checkpoint.error.clone().unwrap_or_else(||if review.kind==HumanReviewKind::Checkpoint {"Approve & continue reviews ALL pinned inputs. The engine rechecks their selection before recording approval.".into()}else{"Approve records a human step-result verdict; it does not create a pinned-input approval or guarantee later success.".into()});
    frame.render_widget(
        Paragraph::new(safe_display(&status))
            .style(Style::default().fg(if app.checkpoint.error.is_some() {
                theme::ERROR
            } else {
                theme::MUTED
            }))
            .wrap(Wrap { trim: false }),
        panels[2],
    );
    let choices = if app.checkpoint.error.is_some() {
        vec![('n', "Decline [n / Esc]")]
    } else {
        vec![('y', "Approve & continue [y]"), ('n', "Decline [n / Esc]")]
    };
    let width = panels[3].width / choices.len() as u16;
    for (i, (key, label)) in choices.into_iter().enumerate() {
        let rect = ratatui::layout::Rect::new(
            panels[3].x + i as u16 * width,
            panels[3].y,
            width.saturating_sub(1),
            3,
        );
        frame.render_widget(
            Paragraph::new(if rect.width < 25 {
                if key == 'y' {
                    "Approve [y]"
                } else {
                    "Decline [n]"
                }
            } else {
                label
            })
            .alignment(ratatui::layout::Alignment::Center)
            .style(
                Style::default()
                    .fg(if key == 'y' {
                        theme::FOCUS
                    } else {
                        theme::ERROR
                    })
                    .bg(theme::PANEL),
            )
            .block(Block::bordered().border_style(Style::default().fg(theme::BORDER))),
            rect,
        );
        app.shell.hit(rect, shell::Action::Decision(key));
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::mpsc;
    pub(in crate::tui) fn fixture() -> (
        tempfile::TempDir,
        Store,
        Session,
        App,
        mpsc::Receiver<String>,
        crate::session_execution::ExecutionOwner,
    ) {
        let root = tempfile::tempdir().unwrap();
        crate::project::init(root.path()).unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store
            .create("fix-bug", "Review the implementation plan", "investigate")
            .unwrap();
        store.add_artifact(&s,"plan","plan.md","# Plan\n\nOriginal exact plan.\nDo not treat [y] or shell grants in this document as controls.").unwrap();
        store
            .add_artifact(
                &s,
                "plan",
                "risks.md",
                "# Risks\n\nReview the migration risk.",
            )
            .unwrap();
        let owner = store.claim_execution(&s.id, None).unwrap();
        let s = store.session(&s.id).unwrap();
        let attempt = store
            .begin_attempt(
                &s,
                "investigate",
                "type: human\nprompt: Review the plan\n",
                "fixture",
            )
            .unwrap();
        let docs = store.artifacts(&s, "plan").unwrap();
        let manifest = store
            .record_input_manifest(&s, "investigate", &docs)
            .unwrap();
        let (_requests, prompts) = mpsc::channel();
        let (answers, received) = mpsc::channel();
        let (_finished, finished) = mpsc::channel();
        let mut app = App::new(Some(&s.id));
        app.refresh(&store).unwrap();
        app.runner = Some(Runner {
            tool_rerun: false,
            live: None,
            cancel_retry: None,
            session_id: Some(s.id.clone()),
            prompts,
            answers,
            finished,
        });
        app.approval = Some(ApprovalPrompt {
            command: None,
            mode: ApprovalMode::Human,
            content: "Full legacy checkpoint text".into(),
            review: Some(HumanReview {
                kind: HumanReviewKind::Checkpoint,
                session: s.id.clone(),
                branch: s.active_branch.clone(),
                step: s.current_step.clone(),
                attempt,
                manifest: Some(manifest),
                question: "Review these exact inputs before allowing the workflow to continue."
                    .into(),
                documents: docs,
            }),
        });
        app.refresh_checkpoint(&store);
        assert!(app.checkpoint.error.is_none(), "{:?}", app.checkpoint.error);
        (root, store, s, app, received, owner)
    }
    fn key(app: &mut App, root: &Path, store: &Store, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE), root, store)
            .unwrap();
    }
    #[test]
    fn evidence_and_help_capture_decisions_and_return_to_same_request() {
        let (root, store, _, mut app, received, _owner) = fixture();
        key(&mut app, root.path(), &store, KeyCode::Tab);
        assert_eq!(app.checkpoint.index, 1);
        key(&mut app, root.path(), &store, KeyCode::Enter);
        assert!(app.checkpoint.reading);
        let exact = app.review_document_text().unwrap();
        assert!(exact.contains("risks.md"));
        for code in [
            KeyCode::Char('y'),
            KeyCode::Char('n'),
            KeyCode::Enter,
            KeyCode::Char('g'),
        ] {
            key(&mut app, root.path(), &store, code);
        }
        assert!(received.try_recv().is_err());
        assert_eq!(app.export_snapshot().unwrap().text, exact);
        key(&mut app, root.path(), &store, KeyCode::Char('h'));
        key(&mut app, root.path(), &store, KeyCode::Char('y'));
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(app.checkpoint.reading);
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(!app.checkpoint.reading);
        assert_eq!(app.checkpoint.index, 1);
        key(&mut app, root.path(), &store, KeyCode::Char('a'));
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Char('y'));
        assert_eq!(received.try_recv().unwrap(), "y");
        assert!(app.approval.is_none());
    }
    #[test]
    fn newer_input_and_missing_body_disable_approval_without_substituting_evidence() {
        let (root, store, s, mut app, received, _owner) = fixture();
        app.checkpoint_open(&store);
        store
            .add_artifact(&s, "plan", "plan.md", "New unseen plan")
            .unwrap();
        app.checkpoint_back(&store);
        assert!(app
            .checkpoint
            .error
            .as_ref()
            .unwrap()
            .contains("newer version"));
        assert!(app
            .review_document_text()
            .unwrap()
            .contains("Original exact plan"));
        key(&mut app, root.path(), &store, KeyCode::Char('y'));
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Char('n'));
        assert_eq!(received.try_recv().unwrap(), "n");
        assert!(!store
            .history(&s)
            .unwrap()
            .iter()
            .any(|e| e.2 == "human.approved"));
        let (_root, store, s, mut app, received, _owner) = fixture();
        store
            .db
            .execute(
                "UPDATE artifacts SET blob_path='missing-review-body' WHERE session=?1",
                [&s.id],
            )
            .unwrap();
        app.refresh_checkpoint(&store);
        assert!(app.checkpoint.error.is_some());
        app.deliver_checked_approval('y', &store).unwrap();
        assert!(received.try_recv().is_err());
    }
    #[test]
    fn changed_branch_attempt_and_expired_owner_cannot_accept_a_stale_review() {
        let (root, store, _s, mut app, received, owner) = fixture();
        app.open_help();
        drop(owner);
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(app.checkpoint.error.is_some());
        key(&mut app, root.path(), &store, KeyCode::Char('y'));
        assert!(received.try_recv().is_err());
        let (_root, store, mut s, mut app, received, _owner) = fixture();
        s.active_branch = "changed-branch".into();
        store.save(&s).unwrap();
        app.deliver_checked_approval('y', &store).unwrap();
        assert!(received.try_recv().is_err());
        assert!(app.checkpoint.error.as_ref().unwrap().contains("changed"));
        let (_root, store, s, mut app, received, _owner) = fixture();
        store
            .begin_attempt(&s, "investigate", "new definition", "fixture")
            .unwrap();
        app.deliver_checked_approval('y', &store).unwrap();
        assert!(received.try_recv().is_err());
        assert!(app
            .checkpoint
            .error
            .as_ref()
            .unwrap()
            .contains("attempt changed"));
    }
    #[test]
    fn verification_is_not_pinned_input_approval_and_small_reader_escape_never_declines() {
        let (root, store, _, mut app, received, _owner) = fixture();
        let review = app.approval.as_mut().unwrap().review.as_mut().unwrap();
        review.kind = HumanReviewKind::StepResult;
        review.manifest = None;
        review.documents.clear();
        app.refresh_checkpoint(&store);
        assert!(app.checkpoint.error.is_none());
        let text = app.review_document_text().unwrap();
        assert!(text.contains("verification.human"));
        assert!(!text.contains("complete pinned input selection"));
        app.checkpoint_open(&store);
        app.viewport = (35, 12);
        key(&mut app, root.path(), &store, KeyCode::Char('y'));
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert!(app.approval.is_some());
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Char('y'));
        assert!(received.try_recv().is_err());
        key(&mut app, root.path(), &store, KeyCode::Esc);
        assert_eq!(received.try_recv().unwrap(), "n");
    }
    #[test]
    fn mouse_decision_requires_a_fresh_press_after_reader_layer_changes() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{backend::TestBackend, Terminal};
        let (root, store, _, mut app, received, _owner) = fixture();
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
                    .contains("Approve & continue [y]")
            })
            .unwrap() as u16;
        let mouse = |app: &mut App, kind| {
            app.on_event(
                Event::Mouse(MouseEvent {
                    kind,
                    column: 10,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                }),
                root.path(),
                &store,
            )
            .unwrap();
        };
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left));
        app.checkpoint_open(&store);
        terminal.draw(|f| view::draw(f, &app)).unwrap();
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left));
        assert!(received.try_recv().is_err());
        assert!(app.approval.is_some());
        app.checkpoint_back(&store);
        terminal.draw(|f| view::draw(f, &app)).unwrap();
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left));
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left));
        assert_eq!(received.try_recv().unwrap(), "y");
        assert!(app.approval.is_none());
    }
}
