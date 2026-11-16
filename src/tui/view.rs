//! Workspace layout and rendering of the current interaction state.
use super::*;

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 40 || area.height < 14 || (app.approval.is_some() && area.height < 20) {
        let text = if app.approval.is_some() {
            "Resize to at least 40×20 to review approval. Decisions disabled. Esc denies."
        } else {
            "Terminal too small. Resize to at least 40×14. Drafts preserved. q quits when idle."
        };
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
        return;
    }
    let compact = area.width < 72 || area.height < 22;
    let outer = Layout::vertical([
        Constraint::Length(if compact { 4 } else { 7 }),
        Constraint::Min(6),
        Constraint::Length(if compact { 3 } else { 4 }),
        Constraint::Length(2),
    ])
    .split(area);
    let (workflow, raw_status, step) = match app.session() {
        Some(session) => (
            session.workflow.as_str(),
            session.status.as_str(),
            session.current_step.as_str(),
        ),
        None => ("No session", "idle", "-"),
    };
    let workspace = app
        .workspace
        .as_ref()
        .filter(|_| app.workspace_session.as_deref() == app.session().map(|s| s.id.as_str()));
    let starting = app
        .runner
        .as_ref()
        .is_some_and(|r| r.session_id.as_deref() == app.session().map(|s| s.id.as_str()))
        && raw_status == "ready";
    let status = if starting {
        "STARTING".into()
    } else if workspace
        .is_some_and(|w| w.primary == crate::workspace_view::PrimaryAction::RecoverOwner)
    {
        "INTERRUPTED".into()
    } else {
        raw_status.to_uppercase()
    };
    let active_tab = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive_tab = Style::default().fg(Color::DarkGray);
    let goal = workspace
        .map(|w| w.goal.as_str())
        .or_else(|| app.session().map(|s| s.goal.as_str()))
        .unwrap_or("Choose a session");
    let attempt = workspace
        .map(|w| w.attempt_label())
        .unwrap_or_else(|| "context unavailable".into());
    let progress = workspace
        .map(|w| w.stepper(step, area.width.saturating_sub(2) as usize))
        .unwrap_or_else(|| "Workflow context unavailable; h history remains available".into());
    let next = if app.approval.is_some() {
        "Decide in the approval pane; Enter never approves"
    } else {
        workspace
            .map(|w| w.primary.label())
            .unwrap_or("Inspect configuration/history")
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(inline_display(&format!(" {goal}"))).style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(inline_display(&format!(
                " {workflow} · {status} · {step} · {attempt}"
            ))),
            Line::from(inline_display(&progress)),
            Line::from(inline_display(&format!(" Next: {next}")))
                .style(Style::default().fg(Color::Yellow)),
            Line::from(vec![
                Span::styled(
                    " [1] Activity ",
                    if app.view == View::Conversation {
                        active_tab
                    } else {
                        inactive_tab
                    },
                ),
                Span::styled(
                    " [2] Artifacts ",
                    if app.view == View::Artifacts {
                        active_tab
                    } else {
                        inactive_tab
                    },
                ),
                Span::styled(
                    " [3] Edits ",
                    if app.view == View::Edits {
                        active_tab
                    } else {
                        inactive_tab
                    },
                ),
                Span::raw(" ? help · C copy · E editor · F filter"),
            ]),
        ])
        .block(border(" Session workspace ", false)),
        outer[0],
    );

    let overlay = app.edits.pending.is_some()
        || (app.result_open && app.view == View::Conversation)
        || app.activity_filtering
        || app.artifact_browser.is_some()
        || app.approval.is_some()
        || app.new_session.is_some()
        || app.searching
        || app.help
        || app.diagnostic.is_some()
        || app.usage_detail.is_some()
        || app.context_detail.is_some()
        || app.restart_plan.is_some()
        || !app.restart_choices.is_empty();
    let widths = if compact {
        if app.focus == Focus::Sessions && !overlay {
            [Constraint::Percentage(100), Constraint::Length(0)]
        } else {
            [Constraint::Length(0), Constraint::Percentage(100)]
        }
    } else {
        [Constraint::Percentage(27), Constraint::Percentage(73)]
    };
    let columns = Layout::horizontal(widths).split(outer[1]);

    let sessions = app.sessions.iter().map(|session| {
        ListItem::new(format!(
            "{}\n{} [{}]\n{}\n{}",
            inline_display(&session.goal),
            inline_display(&session.workflow),
            inline_display(&session.status),
            inline_display(&session.current_step),
            app.session_activity
                .get(&session.id)
                .map(|(_, date)| format!("Last {}Z", date.get(5..16).unwrap_or(date)))
                .unwrap_or_else(|| "Last: not recorded".into())
        ))
    });
    let sessions_list = List::new(sessions)
        .block(
            border(" Sessions · f/n ", app.focus == Focus::Sessions).title_bottom(format!(
                " {}/{} · {} ",
                app.sessions.len(),
                app.session_total,
                inline_display(&app.session_query)
            )),
        )
        .highlight_symbol("▶ ")
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut session_state = ListState::default();
    if !app.sessions.is_empty() {
        session_state.select(Some(app.session_index));
    }
    frame.render_stateful_widget(sessions_list, columns[0], &mut session_state);

    if let Some(prompt) = &app.approval {
        approval::draw(frame, app, prompt, columns[1]);
    } else if let Some(browser) = &app.artifact_browser {
        let panes = if browser.reading {
            Layout::horizontal([Constraint::Length(0), Constraint::Min(1)]).split(columns[1])
        } else {
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(columns[1])
        };
        let versions = browser.versions.iter().map(|v| {
            ListItem::new(format!(
                "{}\n{}",
                inline_display(&format!(
                    "#{} · {}{}",
                    v.event_id,
                    v.step,
                    if app.session().is_some_and(|s| s.active_branch != v.branch) {
                        " · inherited"
                    } else {
                        ""
                    }
                )),
                inline_display(&v.timestamp)
            ))
        });
        let mut selected = ListState::default();
        selected.select(Some(browser.index));
        frame.render_stateful_widget(
            List::new(versions)
                .highlight_symbol("▶ ")
                .highlight_style(Style::default().fg(Color::Cyan))
                .block(border(" Versions · newest first ", !browser.reading)),
            panes[0],
            &mut selected,
        );
        let current = browser.versions.get(browser.index);
        let title = format!(
            " #{} · {:?} · {} · Enter read/list ",
            current.map(|v| v.event_id).unwrap_or(0),
            browser.mode,
            inline_display(&browser.name)
        );
        let source = browser.error.as_deref().unwrap_or(&browser.text);
        let markdown = browser.error.is_none()
            && browser.mode == crate::artifact_history::Mode::Content
            && app.render_markdown;
        reader::draw_document(
            frame,
            app,
            reader::Document {
                tag: format!(
                    "{}:{}:{}:{:?}",
                    browser.session,
                    browser.branch,
                    current.map(|v| v.event_id).unwrap_or(0),
                    browser.mode
                ),
                source,
                markdown,
                headers: Vec::new(),
            },
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(if browser.error.is_some() {
                    Color::Red
                } else {
                    Color::Cyan
                })),
            panes[1],
        );
    } else if app.activity_filtering {
        input_panel(
            frame,
            &app.filter_editor,
            &app.activity_query,
            "Filter activity",
            &format!(
                "{} matching records · Enter apply · Esc clear",
                app.events.len()
            ),
            columns[1],
        );
    } else if app.help {
        draw_scrolled(
            frame,
            app,
            Paragraph::new(HELP)
                .wrap(Wrap { trim: false })
                .block(border(" Keyboard help · Esc closes ", true)),
            columns[1],
        );
    } else if let Some(form) = &app.new_session {
        if let Some(goal) = &form.goal {
            let choice = &form.workflows[form.cursor];
            input_panel(
                frame,
                &app.goal_editor,
                goal,
                " New session · goal ",
                &format!(
                    "{} — {} · Enter starts · Ctrl+J newline · Esc cancel\n{}",
                    choice.id,
                    choice.title,
                    form.error.as_deref().unwrap_or("")
                ),
                columns[1],
            );
        } else {
            let choices = form.workflows.iter().map(|w| {
                ListItem::new(inline_display(&format!(
                    "{} — {}{}",
                    w.id,
                    w.title,
                    if w.error.is_some() { " [invalid]" } else { "" }
                )))
            });
            let mut state = ListState::default();
            if !form.workflows.is_empty() {
                state.select(Some(form.cursor));
            }
            let panels =
                Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).split(columns[1]);
            frame.render_stateful_widget(
                List::new(choices)
                    .highlight_symbol("▶ ")
                    .highlight_style(Style::default().fg(Color::Cyan))
                    .block(border(" New session · choose workflow ", true)),
                panels[0],
                &mut state,
            );
            let help = if form.workflows.is_empty() {
                "No workflows found. Add workflows/<name>/workflow.yaml and step files."
            } else {
                form.error
                    .as_deref()
                    .unwrap_or("↑/↓ choose · Enter set goal · Esc cancel")
            };
            frame.render_widget(
                Paragraph::new(safe_display(help)).wrap(Wrap { trim: false }),
                panels[1],
            );
        }
    } else if app.searching {
        input_panel(
            frame,
            &app.session_editor,
            &app.session_query,
            " Find session ",
            &format!(
                "{} of {} sessions · ↑/↓ select · Enter keep · Esc clear",
                app.sessions.len(),
                app.session_total
            ),
            columns[1],
        );
    } else if let Some(message) = &app.diagnostic {
        draw_scrolled(
            frame,
            app,
            Paragraph::new(crate::markdown::render(message))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" ATTENTION — Esc closes; arrows scroll ")
                        .border_style(Style::default().fg(Color::Red)),
                ),
            columns[1],
        );
    } else if let Some(message) = &app.usage_detail {
        draw_scrolled(
            frame,
            app,
            Paragraph::new(crate::markdown::render(message))
                .wrap(Wrap { trim: false })
                .block(border(" Usage — session spending · Esc closes ", true)),
            columns[1],
        );
    } else if let Some(message) = &app.context_detail {
        draw_scrolled(
            frame,
            app,
            Paragraph::new(crate::markdown::render(&safe_display(message)))
                .wrap(Wrap { trim: false })
                .block(border(" Attempt context · Esc closes ", true)),
            columns[1],
        );
    } else if app.restart_plan.is_some() || !app.restart_choices.is_empty() {
        recovery::draw(frame, app, columns[1]);
    } else if app.edits.pending.is_some() || app.view == View::Edits {
        edits::draw(frame, app, columns[1]);
    } else if app.result_open && app.view == View::Conversation {
        draw_scrolled(
            frame,
            app,
            Paragraph::new(crate::markdown::render(&safe_display(&app.result_text)))
                .wrap(Wrap { trim: false })
                .block(border(
                    " Workflow result · s result · 3 edits · h activity ",
                    true,
                )),
            columns[1],
        );
    } else if app.view == View::Artifacts {
        let panes = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(columns[1]);
        let items = app.artifacts.iter().map(|artifact| {
            ListItem::new(format!(
                "{}\n  step: {}  ·  {} bytes",
                inline_display(&artifact.name),
                inline_display(&artifact.step),
                artifact.content.len()
            ))
        });
        let artifacts = List::new(items)
            .block(
                border(" Artifacts · v versions ", app.focus == Focus::Artifacts)
                    .title(format!(" {} saved ", app.artifacts.len())),
            )
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        let mut state = ListState::default();
        if !app.artifacts.is_empty() {
            state.select(Some(app.artifact_index));
        }
        frame.render_stateful_widget(artifacts, panes[0], &mut state);

        let title = if app.render_markdown {
            " Artifact · Markdown · Enter returns · m raw "
        } else {
            " Artifact · Raw text · Enter returns · m Markdown "
        };
        if let Some(artifact) = app.artifact() {
            let headers = vec![
                Line::styled(
                    inline_display(&artifact.name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::styled(
                    format!(
                        "Produced by: {}  ·  {} bytes",
                        inline_display(&artifact.step),
                        artifact.content.len()
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
                Line::default(),
            ];
            let tag = format!(
                "{}:{}:{}",
                app.session().map(|s| s.id.as_str()).unwrap_or(""),
                app.session()
                    .map(|s| s.active_branch.as_str())
                    .unwrap_or(""),
                artifact.event_id
            );
            reader::draw_document(
                frame,
                app,
                reader::Document {
                    tag,
                    source: &artifact.content,
                    markdown: app.render_markdown,
                    headers,
                },
                border(title, app.focus == Focus::Detail),
                panes[1],
            );
        } else {
            draw_scrolled(frame,app,Paragraph::new("No artifacts on this branch yet.\n\nArtifact actions and put-artifact register documents for later steps and human review.").wrap(Wrap{trim:false}).block(border(title,app.focus==Focus::Detail)),panes[1]);
        }
    } else {
        let live = app
            .runner
            .as_ref()
            .filter(|r| r.session_id.as_deref() == app.session().map(|s| s.id.as_str()))
            .and_then(|r| r.live.as_ref());
        if let Some(live) = live {
            let panels = Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(columns[1]);
            activity::draw(frame, app, workspace, panels[0]);
            let live = live.lock().unwrap();
            let status = if live.state.is_empty() {
                "Starting workflow…"
            } else {
                &live.state
            };
            let body = if live.text.is_empty() {
                format!("{status}\nOutput will appear as the provider sends it. Tool and human approvals remain interactive.")
            } else {
                safe_display(&live.text)
            };
            let paragraph = Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Color::Yellow))
                .block(
                    border(" Live model output · unvalidated preview ", false).title_bottom(
                        inline_display(&format!(
                            "{} · {}{}",
                            live.model,
                            status,
                            if live.truncated {
                                " · showing latest output"
                            } else {
                                ""
                            }
                        )),
                    ),
                );
            let offset = paragraph
                .line_count(panels[1].width.saturating_sub(2))
                .saturating_sub(panels[1].height.saturating_sub(2) as usize)
                .min(u16::MAX as usize) as u16;
            frame.render_widget(paragraph.scroll((offset, 0)), panels[1]);
        } else {
            activity::draw(frame, app, workspace, columns[1]);
        }
    }

    let input_title = if app.manager_job.is_some() {
        " Manager answering · /cancel-manager stops future retries "
    } else if app.manager_enabled {
        " Message manager · read-only · /resume runs agent "
    } else if app.runner.is_some() {
        " Agent running · x stops future provider requests "
    } else {
        " Message agent · i to focus · Enter sends "
    };
    let composer_block = border(input_title, app.focus == Focus::Compose && !overlay);
    let input_area = composer_block.inner(outer[2]);
    frame.render_widget(composer_block, outer[2]);
    app.composer_editor.draw(
        frame,
        &app.composer,
        if app.session().is_some_and(|s| s.status == "completed") {
            "Run ended · s result · 3 edits/revert · /ask QUESTION"
        } else {
            "Type instructions to resume this session…"
        },
        input_area,
        app.focus == Focus::Compose && !overlay,
    );

    let footer = if app.edits.pending.is_some() {
        "Revert preview · y applies / n or Esc cancels · Enter does nothing · ↑/↓ scroll".into()
    } else if app.result_open && app.view == View::Conversation {
        "Run ended · report is agent-authored · 2 artifacts · 3 edits/revert · h activity · ↑/↓ scroll".into()
    } else if app.view == View::Edits && !overlay {
        "Recorded native edits · ↑/↓ files · Enter read/list · u revert file · U revert all (preview first)".into()
    } else if app.approval.is_some() {
        "Press a decision key · Esc denies · Enter does nothing · ↑/↓ scroll request".into()
    } else if app.activity_filtering {
        "Type to filter activity · Enter keep · Esc clear".into()
    } else if app.artifact_browser.is_some() {
        "↑/↓ select/scroll · Enter read/list · p provenance · d diff · o content · m raw · Esc close".into()
    } else if app.help {
        "↑/↓ or j/k scroll · PgUp/PgDn page · Home/End · Esc close".into()
    } else if app.new_session.is_some() {
        "New session · Enter chooses workflow / starts goal · Ctrl+J newline · arrows edit · Esc cancel".into()
    } else if app.searching {
        "Search · type to filter · Enter keeps filter · Esc clears · ↑/↓ select".into()
    } else if app.diagnostic.is_some() {
        "Attention required · ↑/↓ scroll full diagnostic · Esc close · inspect evidence before retrying".into()
    } else if app.context_detail.is_some() || app.usage_detail.is_some() {
        "↑/↓ scroll · Esc close".into()
    } else if app.restart_plan.is_some() {
        "Restore uses recorded edits · y confirm / n cancel · ↑/↓ scroll preview".into()
    } else if !app.restart_choices.is_empty() {
        "j/k choose step  ·  Enter preview  ·  Esc cancel".into()
    } else {
        format!(
            "{}   │   ? help · C copy · E editor · F filter · f sessions · i message",
            app.notice
        )
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{}\n{footer}",
            if compact && app.approval.is_some() {
                "Approval: explicit keys only"
            } else if compact {
                "1 Activity · 2 Artifacts · 3 Edits · s Result"
            } else {
                &app.usage_line
            }
        )),
        outer[3],
    );
}

fn input_panel(
    frame: &mut Frame,
    editor: &editor::Editor,
    text: &str,
    title: &'static str,
    hint: &str,
    area: ratatui::layout::Rect,
) {
    let block = border(title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let panels = Layout::vertical([
        Constraint::Length(3.min(inner.height.saturating_sub(1))),
        Constraint::Min(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(safe_display(hint))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false }),
        panels[0],
    );
    editor.draw(frame, text, "Type here…", panels[1], true);
}
