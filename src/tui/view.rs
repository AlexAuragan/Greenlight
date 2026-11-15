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
    let status = if workspace
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
            Line::from(inline_display(&format!(" {goal}")))
                .style(Style::default().add_modifier(Modifier::BOLD)),
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
                Span::raw(" ? help · C copy · E editor · F filter"),
            ]),
        ])
        .block(border(" Session workspace ", false)),
        outer[0],
    );

    let overlay = app.activity_filtering
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
            " #{} · {:?} · {} ",
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
        let message=format!("Filter activity: {}▌\n\n{} matching records.\nMatches event kind, step, card title or summary.\nEnter keeps this filter; Esc clears it.\nFull payload search is available in exported history.",app.activity_query,app.events.len());
        frame.render_widget(
            Paragraph::new(safe_display(&message))
                .wrap(Wrap { trim: false })
                .block(border(" Filter activity · F ", true)),
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
            let content=format!("Workflow: {} — {}\n\nGoal:\n{}▌\n\nEnter creates a ready session. No model requests run yet.\nThen use i and /resume or instructions to run.\nEsc cancels.\n\n{}",choice.id,choice.title,goal,form.error.as_deref().unwrap_or(""));
            frame.render_widget(
                Paragraph::new(safe_display(&content))
                    .wrap(Wrap { trim: false })
                    .block(border(" New session · goal ", true)),
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
        let content=format!("Search: {}▌\n\n{} of {} sessions\nMatches goal, workflow, step, status or full ID.\nBlocked sessions first; then latest recorded activity.\n\n↑/↓ select · Enter keep filter · Esc clear\nAfter searching: n new session · Right open selected session",app.session_query,app.sessions.len(),app.session_total);
        frame.render_widget(
            Paragraph::new(safe_display(&content))
                .wrap(Wrap { trim: false })
                .block(border(" Find session ", true)),
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
            " Artifact · Markdown · ↑/↓ scroll · m raw "
        } else {
            " Artifact · Raw text · ↑/↓ scroll · m Markdown "
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
        activity::draw(frame, app, workspace, columns[1]);
    }

    let input = if app.composer.is_empty() {
        "Type instructions to resume this session…".to_owned()
    } else {
        app.composer.clone()
    };
    let input = if app.focus == Focus::Compose {
        format!("{input}▌")
    } else {
        input
    };
    let input_title = if app.manager_job.is_some() {
        " Manager answering · /cancel-manager stops future retries "
    } else if app.manager_enabled {
        " Message manager · read-only · /resume runs agent "
    } else if app.runner.is_some() {
        " Agent running · x stops future provider requests "
    } else {
        " Message agent · i or Enter to focus · Enter sends "
    };
    let composer_style = if app.composer.is_empty() {
        Style::default().fg(Color::Rgb(85, 85, 85))
    } else {
        Style::default()
    };
    let composer = Paragraph::new(input)
        .style(composer_style)
        .wrap(Wrap { trim: false })
        .block(border(input_title, app.focus == Focus::Compose));
    let lines = composer.line_count(outer[2].width.saturating_sub(2));
    let offset = lines
        .saturating_sub(outer[2].height as usize)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(composer.scroll((offset, 0)), outer[2]);

    let footer = if app.approval.is_some() {
        "Press a decision key · Esc denies · Enter does nothing · ↑/↓ scroll request".into()
    } else if app.activity_filtering {
        "Type to filter activity · Enter keep · Esc clear".into()
    } else if app.artifact_browser.is_some() {
        "↑/↓ select/scroll · Enter read · Left list · p provenance · d diff · o content · m raw · Esc close".into()
    } else if app.help {
        "↑/↓ or j/k scroll · PgUp/PgDn page · Home/End · Esc close".into()
    } else if app.new_session.is_some() {
        "New session · Enter continues · Esc cancels · no execution until resume".into()
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
                "? help · C copy · E editor · F filter"
            } else {
                &app.usage_line
            }
        )),
        outer[3],
    );
}
