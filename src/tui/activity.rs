//! Recorded activity presentation and evidence inspection.
use super::*;

impl App {
    pub(super) fn inspect_blocker(&mut self) {
        if let Some(id) = self
            .workspace
            .as_ref()
            .and_then(|w| w.blocker.as_ref())
            .map(|b| b.event_id)
        {
            if let Some(index) = self.events.iter().position(|e| e.id == id) {
                self.event_index = index;
            }
        }
        self.follow_activity = false;
        self.blocker_card = false;
        self.view = View::Conversation;
        self.focus = Focus::Detail;
        self.detail_scroll = 0;
    }
    pub(super) fn toggle_verbose(&mut self) {
        self.blocker_card = false;
        self.show_all = !self.show_all;
        self.diff = None;
        self.detail_scroll = 0;
    }
    pub(super) fn toggle_diff(&mut self, store: &Store) -> Result<()> {
        if self.diff.is_some() {
            self.diff = None;
            self.detail_scroll = 0;
            return Ok(());
        }
        let event = match self.event() {
            Some(event) if matches!(event.kind.as_str(), "file.changed" | "workspace.restored") => {
                event
            }
            _ => return Ok(()),
        };
        let session = self.session().context("no selected session")?;
        let edit = store
            .workspace_edits(session)?
            .into_iter()
            .find(|entry| entry.event_id == event.id)
            .context("edit snapshots missing for selected event")?;
        let mut output = changes::unified(
            &edit.path,
            edit.before.as_deref(),
            edit.after.as_deref().unwrap_or(""),
        );
        if edit.after.is_none() {
            output = output.replacen(&format!("+++ b/{}\n", edit.path), "+++ /dev/null\n", 1);
        }
        self.diff = Some((event.id, output));
        self.follow_activity = false;
        self.blocker_card = false;
        self.focus = Focus::Detail;
        self.detail_scroll = 0;
        Ok(())
    }
    pub(super) fn open_card_artifact(&mut self, store: &Store) -> Result<()> {
        let Some(event) = self.event().filter(|e| e.kind == "artifact.created") else {
            return Ok(());
        };
        let session = self.session().context("No session selected")?;
        let mut browser = crate::artifact_history::Browser::open(store, session, &event.data)?;
        browser.index = browser
            .versions
            .iter()
            .position(|v| v.event_id == event.id)
            .context("artifact creation event is no longer branch-visible")?;
        browser.reading = true;
        browser.load(store, session);
        self.artifact_browser = Some(browser);
        self.follow_activity = false;
        self.detail_scroll = 0;
        Ok(())
    }
    fn card<'a>(&'a self, event: &EventRow) -> std::borrow::Cow<'a, crate::activity_cards::Card> {
        if event.kind != "event.invalid" {
            if let Some(card) = self.activity_cards.cards.get(&event.id) {
                return std::borrow::Cow::Borrowed(card);
            }
        }
        std::borrow::Cow::Owned(crate::activity_cards::describe(
            event.id,
            &event.kind,
            &event.data,
            self.activity_cards.tools.get(&event.id),
        ))
    }
    pub(super) fn detail(&self) -> String {
        if let Some((id, diff)) = &self.diff {
            if self.event().is_some_and(|event| event.id == *id) {
                return diff.clone();
            }
        }
        match self.event() {
            Some(event) if !self.show_all => {
                let tool = self.activity_cards.tools.get(&event.id);
                let card = self.card(event);
                let mut detail = format!(
                    "{}\n{}\n\nEvent #{} · step {} · {}\n",
                    card.title, card.summary, event.id, event.step, event.kind
                );
                if let Some(tool) = tool {
                    detail.push_str(&format!(
                        "\nRecorded request #{}:\n{}\n\nRecorded output #{}:\n{}",
                        tool.metadata.request_event, tool.request, event.id, event.data
                    ));
                } else if let Some(evidence) = self.activity_cards.titles.get(&event.id) {
                    detail.push_str(&format!(
                        "\nModel-authored intent only. This does not establish that the requested action ran or succeeded.\n\nRecorded annotation:\n{}",
                        event.data
                    ));
                    detail.push_str(&format!(
                        "\n\nRecorded model response #{}:\n{}",
                        evidence.annotation.model_event, evidence.response
                    ));
                } else {
                    detail.push_str(&format!("\nRaw recorded evidence:\n{}", event.data));
                }
                if event.kind == "artifact.created" {
                    detail.push_str("\n\no: open this exact artifact version; p in its version browser shows provenance.");
                }
                if matches!(event.kind.as_str(), "file.changed" | "workspace.restored") {
                    detail.push_str("\n\nd: inspect the recorded journal diff.");
                }
                detail
            }
            Some(event) => format!(
                "Event #{}  ·  {}  ·  {}\n\n{}",
                event.id, event.step, event.kind, event.data
            ),
            None if !self.activity_query.is_empty() => {
                "No activity matches this filter. Press F then Esc to clear it.".into()
            }
            None => "No events. Start a workflow from another terminal.".into(),
        }
    }
}

pub(super) fn is_chatter(kind: &str) -> bool {
    crate::activity_cards::is_protocol(kind)
}

pub(super) fn conversational_label(kind: &str) -> &str {
    match kind {
        "user.instruction" => "You",
        "activity.title" => "Agent intent (model-authored)",
        "step.started" => "Step started",
        "step.completed" => "Step completed",
        "step.blocked" => "Blocked",
        "step.error" => "Error",
        "event.invalid" => "Malformed event",
        "workspace.restore-failed" => "Restore failed",
        "workspace.restore-completed" => "Restore completed",
        "runner.error" => "Worker error",
        "step.deviation" => "Needs attention",
        "verification.failed" => "Verification failed",
        "provider.failure" => "Provider failure",
        "provider.retry" => "Waiting for provider retry (x cancels)",
        "provider.retry-stopped" | "provider.retry-cancelled" => "Provider retry stopped",
        "tool.error" => "Tool failed",
        "tool.result" => "Tool output",
        "file.changed" => "Code edit",
        "workspace.restored" => "Restored",
        "permission.decision" => "Approval",
        "artifact.created" => "Artifact",
        "branch.forked" => "Restart",
        other => other,
    }
}

pub(super) fn invalid_event_payload(kind: &str, data: &str) -> Option<String> {
    let result: Result<()> = (|| {
        match kind {
            "workspace.manifest.before" | "workspace.manifest.after" => {
                serde_json::from_str::<crate::workspace::Manifest>(data)?;
            }
            "workspace.delta" => {
                serde_json::from_str::<Vec<crate::workspace::FileDifference>>(data)?;
            }
            "context.inputs" | "human.approved" => {
                crate::input_pins::validate_recorded_event(kind, data)?;
            }
            "tool.finished" => {
                serde_json::from_str::<crate::activity_cards::ToolFinished>(data)?;
            }
            "activity.title" => {
                serde_json::from_str::<crate::activity_titles::Annotation>(data)?;
            }
            "verification.evidence" => {
                let value: serde_json::Value = serde_json::from_str(data)?;
                if !value["attempt"].is_string()
                    || !value["artifact_events"]
                        .as_array()
                        .is_some_and(|a| a.iter().all(|v| v.is_i64()))
                    || !value["observation_events"]
                        .as_array()
                        .is_some_and(|a| a.iter().all(|v| v.is_i64()))
                {
                    bail!("invalid verification evidence fields");
                }
            }
            "workspace.restore-failed" => {
                let value: serde_json::Value = serde_json::from_str(data)?;
                if !value["error"].is_string()
                    || !value["changed_files"].is_u64()
                    || !value["journaled_files"].is_u64()
                {
                    bail!("invalid restore failure fields");
                }
            }
            _ => {} // Unknown event types remain readable as raw evidence.
        }
        Ok(())
    })();
    result.err().map(|error| format!("{error:#}"))
}

pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    workspace: Option<&crate::workspace_view::SessionWorkspace>,
    area: ratatui::layout::Rect,
) {
    if app.blocker_card
        && workspace.is_some_and(|w| {
            w.blocker.is_some()
                || matches!(
                    w.primary,
                    crate::workspace_view::PrimaryAction::RecoverOwner
                        | crate::workspace_view::PrimaryAction::RecoverWrites
                )
        })
    {
        let workspace = workspace.unwrap();
        let mut message = format!("{}\n{}\n\n", workspace.objective, workspace.attempt_label());
        if let Some(blocker) = &workspace.blocker {
            message.push_str(&format!(
                "{}\n\nEvidence: event #{}\n\n",
                blocker.reason, blocker.event_id
            ));
        }
        message.push_str(&format!("Next: {}\n\ne: inspect evidence\nh: activity history\nc: exact context and version references\n",workspace.primary.label()));
        draw_scrolled(
            frame,
            app,
            Paragraph::new(crate::markdown::render(&safe_display(&message)))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Needs attention — evidence before recovery ")
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
            area,
        );
    } else {
        let right = Layout::vertical([Constraint::Length(4), Constraint::Min(3)]).split(area);
        let context = workspace
            .map(|w| {
                format!(
                    "{}\nOutputs: {} · verification: {}",
                    w.objective,
                    w.outputs
                        .as_ref()
                        .map(|v| if v.is_empty() {
                            "none declared".into()
                        } else {
                            v.join(", ")
                        })
                        .unwrap_or_else(|| "unknown".into()),
                    w.verification
                )
            })
            .unwrap_or_else(|| {
                app.workspace_error
                    .clone()
                    .unwrap_or_else(|| "Choose a session".into())
            });
        frame.render_widget(
            Paragraph::new(safe_display(&context))
                .wrap(Wrap { trim: false })
                .block(border(" Step context · c for exact records ", false)),
            right[0],
        );
        let show_detail = app.focus == Focus::Detail
            || (app.focus == Focus::Compose && app.focus_before_compose == Focus::Detail);
        if show_detail {
            let detail = app.detail();
            let title = if app.diff.is_some() {
                " Recorded diff · Left returns "
            } else if app.show_all {
                " Raw evidence · Left returns "
            } else {
                " Activity card · Left returns "
            };
            reader::draw_document(
                frame,
                app,
                reader::Document {
                    tag: format!(
                        "activity:{}:{}:{}",
                        app.session()
                            .map(|s| s.active_branch.as_str())
                            .unwrap_or(""),
                        app.event().map(|e| e.id).unwrap_or(0),
                        app.diff.is_some()
                    ),
                    source: &detail,
                    markdown: false,
                    headers: Vec::new(),
                },
                border(title, true),
                right[1],
            );
        } else {
            let compact = right[1].height < 6 || right[1].width < 36;
            let events = app.events.iter().map(|event| {
                if app.show_all {
                    let item = ListItem::new(inline_display(&format!(
                        "#{} {}: {}",
                        event.id,
                        conversational_label(&event.kind),
                        event.data.lines().next().unwrap_or("")
                    )));
                    return if matches!(
                        event.kind.as_str(),
                        "event.invalid"
                            | "step.error"
                            | "runner.error"
                            | "workspace.restore-failed"
                            | "verification.failed"
                            | "tool.error"
                    ) {
                        item.style(Style::default().fg(Color::Red))
                    } else {
                        item
                    };
                }
                let card = app.card(event);
                let color = match card.tone {
                    crate::activity_cards::Tone::Info => Color::Cyan,
                    crate::activity_cards::Tone::Good => Color::Green,
                    crate::activity_cards::Tone::Attention => Color::Yellow,
                    crate::activity_cards::Tone::Error => Color::Red,
                };
                let title = Line::styled(
                    inline_display(&format!("#{} {}", event.id, card.title)),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                );
                let summary = Line::styled(
                    inline_display(&format!("  {} · {}", event.step, card.summary)),
                    Style::default().fg(Color::DarkGray),
                );
                ListItem::new(if compact {
                    vec![title]
                } else {
                    vec![title, summary]
                })
            });
            let list = List::new(events)
                .block(
                    border(
                        if app.show_all && app.follow_activity {
                            " Activity · full · following "
                        } else if app.show_all {
                            " Activity · full · paused · End follows "
                        } else if app.follow_activity {
                            " Cards · following · Enter expands "
                        } else {
                            " Cards · paused · End follows "
                        },
                        app.focus == Focus::Events,
                    )
                    .title_bottom(format!(
                        " F filter: {} · {} records ",
                        inline_display(&app.activity_query),
                        app.events.len()
                    )),
                )
                .highlight_symbol("▶ ")
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                );
            let mut selected = ListState::default();
            if !app.events.is_empty() {
                selected.select(Some(app.event_index));
            }
            frame.render_stateful_widget(list, right[1], &mut selected);
        }
    }
}
