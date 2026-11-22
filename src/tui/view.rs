//! Workspace layout and rendering of the current interaction state.
use super::*;

pub(super) fn draw(frame: &mut Frame, app: &App) {
    app.begin_frame();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme::TEXT).bg(theme::BACKGROUND)),
        frame.area(),
    );
    draw_workspace(frame, app);
    if app.permission_popup_visible() && !app.help {
        approval::draw_popup(frame, app);
        return;
    }
    if (app.help || app.shell.more) && (frame.area().width < 4 || frame.area().height < 4) {
        app.shell.hits.borrow_mut().clear();
        return;
    }
    if app.shell.more && !app.help {
        app.shell.hits.borrow_mut().clear();
        let actions = app.more_actions();
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(38);
        let height = (actions.len() as u16 + 3).min(area.height.saturating_sub(2));
        let popup = ratatui::layout::Rect::new(
            (area.width - width) / 2,
            (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(ratatui::widgets::Clear, popup);
        frame.render_widget(border(" Actions · Esc closes ", true), popup);
        let start = app
            .shell
            .menu_index
            .saturating_sub(height.saturating_sub(3) as usize);
        for (row, (index, action)) in actions
            .into_iter()
            .enumerate()
            .skip(start)
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            shell::button(
                frame,
                app,
                ratatui::layout::Rect::new(
                    popup.x + 1,
                    popup.y + 1 + row as u16,
                    width.saturating_sub(2),
                    1,
                ),
                action,
                index == app.shell.menu_index,
            );
        }
    }
    if app.help {
        app.shell.hits.borrow_mut().clear();
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(90);
        let height = area.height.saturating_sub(2).min(22);
        let popup = ratatui::layout::Rect::new(
            (area.width - width) / 2,
            (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(ratatui::widgets::Clear, popup);
        let regions = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(popup);
        let context = format!(
            "Keyboard help · {} · mouse {}\n\n{}",
            if app.approval_visible() {
                "Approval pending; decisions disabled"
            } else if app.details.page.is_some() {
                "Run details / usage; read-only"
            } else if app.view == View::Edits {
                "Recorded edits"
            } else {
                "Workspace"
            },
            if app.shell.mouse { "on" } else { "off" },
            help_text()
        );
        draw_scrolled(
            frame,
            app,
            Paragraph::new(context)
                .wrap(Wrap { trim: false })
                .block(border(" Help · Esc returns ", true)),
            regions[0],
        );
        let mut x = regions[1].x;
        for (index, action) in [shell::Action::Close, shell::Action::Mouse]
            .into_iter()
            .enumerate()
        {
            let width = (action.label().len() as u16 + 3).min(regions[1].right().saturating_sub(x));
            shell::button(
                frame,
                app,
                ratatui::layout::Rect::new(x, regions[1].y, width, 1),
                action,
                app.shell.help_actions && app.shell.help_action_index == index,
            );
            x += width;
        }
    }
}
fn draw_workspace(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 40
        || area.height < 14
        || (((app.approval_visible() && !app.permission_popup_visible())
            || app.edits.pending.is_some())
            && area.height < 20)
    {
        let text = if app.approval_visible() {
            "Resize to at least 40×20 to review approval. Decisions disabled. Esc denies."
        } else if app.edits.pending.is_some() {
            "Resize to at least 40×20 to review the reverse diff. Revert disabled. Esc cancels."
        } else {
            "Terminal too small. Resize to at least 40×14. Drafts preserved. q quits when idle."
        };
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
        return;
    }
    let outer = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(if app.composer_active() && area.height < 22 {
            0
        } else {
            3
        }),
        Constraint::Length(if app.composer_active() {
            if area.height < 22 {
                area.height.saturating_sub(8)
            } else {
                10.min(area.height.saturating_sub(11))
            }
        } else {
            0
        }),
        Constraint::Length(4),
    ])
    .split(area);
    let (_workflow, raw_status, step) = match app.session() {
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
    let permission_pending_here = app
        .approval
        .as_ref()
        .is_some_and(|p| p.mode != ApprovalMode::Human)
        && app
            .runner
            .as_ref()
            .is_some_and(|r| r.session_id.as_deref() == app.session().map(|s| s.id.as_str()));
    let status = if permission_pending_here && app.provider_retry_pending() {
        "PROVIDER REQUEST PAUSED".into()
    } else if permission_pending_here {
        "WAITING FOR PERMISSION".into()
    } else if raw_status == "instruction_review" {
        "INSTRUCTION REVIEW".into()
    } else if starting {
        "STARTING".into()
    } else if workspace
        .is_some_and(|w| w.primary == crate::workspace_view::PrimaryAction::RecoverOwner)
    {
        "INTERRUPTED".into()
    } else if workspace.is_some_and(|w| w.primary == crate::workspace_view::PrimaryAction::Observe)
        && !app
            .runner
            .as_ref()
            .is_some_and(|r| r.session_id.as_deref() == app.session().map(|s| s.id.as_str()))
    {
        "OBSERVING".into()
    } else {
        raw_status.to_uppercase()
    };
    let state_color = match status.as_str() {
        "RUNNING" | "STARTING" => theme::FOCUS,
        "BLOCKED"
        | "INTERRUPTED"
        | "INSTRUCTION REVIEW"
        | "WAITING FOR PERMISSION"
        | "PROVIDER REQUEST PAUSED" => theme::ATTENTION,
        _ => theme::MUTED,
    };
    let state = if raw_status == "completed" && !permission_pending_here {
        format!("● Finished · {}", inline_display(step))
    } else {
        format!("● {} · {}", status.to_lowercase(), inline_display(step))
    };
    let state_width = unicode_width::UnicodeWidthStr::width(state.as_str()) as u16;
    let title = format!("Greenlight / {}", inline_display(&app.shell.project));
    let left_width = outer[0].width.saturating_sub(state_width + 2);
    frame.render_widget(
        Paragraph::new(fit(&title, left_width as usize))
            .style(Style::default().add_modifier(Modifier::BOLD)),
        ratatui::layout::Rect::new(outer[0].x, outer[0].y, left_width, 1),
    );
    frame.render_widget(
        Paragraph::new(state).style(Style::default().fg(state_color)),
        ratatui::layout::Rect::new(
            outer[0].right().saturating_sub(state_width),
            outer[0].y,
            state_width.min(outer[0].width),
            1,
        ),
    );
    if let Some(project) = &app.project {
        if app.other_project_owner() || project.interrupted {
            let owner = project
                .session
                .as_ref()
                .map(|s| {
                    format!(
                        "{} · {}",
                        inline_display(&s.goal),
                        inline_display(&s.current_step)
                    )
                })
                .unwrap_or_else(|| "control operation in progress".into());
            let controls = if app.other_project_owner() && project.session.is_some() {
                "[O] Open owner · [R] Refresh"
            } else {
                "[R] Refresh"
            };
            let label = format!(
                "{} · {owner} · {controls}",
                if project.interrupted {
                    "Project interrupted"
                } else {
                    "Project busy"
                }
            );
            let row = ratatui::layout::Rect::new(outer[0].x, outer[0].y + 1, outer[0].width, 1);
            frame.render_widget(
                Paragraph::new(fit(&label, row.width as usize))
                    .style(Style::default().fg(theme::ATTENTION)),
                row,
            );
        }
    }
    let mut x = outer[0].x;
    for (action, view) in [
        (shell::Action::Activity, View::Conversation),
        (shell::Action::Artifacts, View::Artifacts),
        (shell::Action::Edits, View::Edits),
    ] {
        if app.session().is_none() {
            continue;
        }
        let width = action.label().len() as u16 + 1;
        shell::tab(
            frame,
            app,
            ratatui::layout::Rect::new(x, outer[0].y + 2, width, 1),
            action,
            app.view == view,
        );
        x += width;
    }
    let header_actions = app.header_actions();
    let total = header_actions
        .iter()
        .map(|a| a.label().len() as u16 + 3)
        .sum::<u16>();
    let footer_len = app.footer_actions().len();
    {
        let header_y = if total + x < outer[0].right() {
            outer[0].y + 2
        } else {
            outer[0].y + 3
        };
        x = outer[0].right().saturating_sub(total);
        for (index, action) in header_actions.into_iter().enumerate() {
            let width = action.label().len() as u16 + 3;
            shell::button(
                frame,
                app,
                ratatui::layout::Rect::new(x, header_y, width, 1),
                action,
                app.shell.actions_focus && app.shell.action_index == footer_len + index,
            );
            x += width;
        }
    }

    let overlay = app.details.page.is_some()
        || app.edits.pending.is_some()
        || (app.result_open && app.view == View::Conversation)
        || app.activity_filtering
        || app.artifact_browser.is_some()
        || app.approval_visible()
        || app.new_session.is_some()
        || app.searching
        || app.help
        || app.diagnostic.is_some()
        || app.context_detail.is_some()
        || app.restart_plan.is_some()
        || !app.restart_choices.is_empty();
    let show_sessions = app.shell.sessions;
    let overlay = overlay && !show_sessions;
    let widths = if !show_sessions || overlay {
        [Constraint::Length(0), Constraint::Percentage(100)]
    } else if area.width < 100 {
        [Constraint::Percentage(100), Constraint::Length(0)]
    } else {
        [Constraint::Length(40), Constraint::Min(1)]
    };
    let columns = Layout::horizontal(widths).split(outer[1]);

    if show_sessions {
        navigation::draw(frame, app, columns[0]);
    }
    if overlay {
        app.shell.hits.borrow_mut().clear();
    }
    if let Some(prompt) = app
        .approval
        .as_ref()
        .filter(|_| app.approval_visible() && !app.permission_popup_visible())
    {
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
                    "{}{}",
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
                .block(border(" Saved versions ", !browser.reading)),
            panes[0],
            &mut selected,
        );
        let current = browser.versions.get(browser.index);
        let title = format!(" {} · {:?} ", inline_display(&browser.name), browser.mode);
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
                .title(title)
                .padding(ratatui::widgets::Padding::uniform(1))
                .style(
                    Style::default()
                        .bg(theme::PANEL)
                        .fg(if browser.error.is_some() {
                            theme::ERROR
                        } else {
                            theme::TEXT
                        }),
                ),
            panes[1],
        );
    } else if app.activity_filtering {
        input_panel(
            frame,
            app,
            &app.filter_editor,
            &app.activity_query,
            "Filter activity",
            &format!(
                "{} matching records · Enter apply · Esc clear",
                app.events.len()
            ),
            columns[1],
        );
    } else if app.new_session.is_some() {
        compose::draw_creation(frame, app, columns[1]);
    } else if let Some(message) = &app.diagnostic {
        if app.diagnostic_detail {
            reader::draw_document(
                frame,
                app,
                reader::Document {
                    tag: format!("diagnostic:{}", app.diagnostic_revision),
                    source: message,
                    markdown: false,
                    headers: vec![
                        Line::styled(
                            "Full diagnostic",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Line::default(),
                    ],
                },
                Block::default()
                    .padding(ratatui::widgets::Padding::uniform(1))
                    .style(Style::default().bg(theme::PANEL)),
                columns[1],
            );
        } else {
            attention::draw(
                frame,
                app,
                columns[1],
                if app.diagnostic_attention {
                    app.attention.as_ref()
                } else {
                    None
                },
            );
        }
    } else if app.details.page.is_some() {
        details::draw(frame, app, columns[1]);
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
    } else if app.navigation.error.is_some()
        || app.navigation.load_error.is_some()
        || app.navigation.missing.is_some()
        || app.session().is_none()
    {
        let text = if let Some(e) = &app.navigation.error {
            format!("Session discovery unavailable\n\n{e}\n\nRetry discovery; no new session or workflow was started.")
        } else if let Some(e) = &app.navigation.load_error {
            format!("Opened session data unavailable\n\n{e}\n\nRetry or use Sessions to choose another run. No replacement was opened.")
        } else if let Some(e) = &app.navigation.missing {
            format!("Opened session unavailable\n\n{e}\n\nSessions opens the browser; choose and Open another session.")
        } else if let Some(e) = &app.navigation.setup {
            format!("Project setup needs attention\n\n{e}\n\nNo sessions in this project.")
        } else {
            "No sessions in this project\n\nChoose New session to select a workflow and enter a goal.\nSessions opens project history; Help explains the controls.".into()
        };
        frame.render_widget(
            Paragraph::new(safe_display(&text))
                .style(Style::default().fg(
                    if app.navigation.error.is_some() || app.navigation.load_error.is_some() {
                        theme::ERROR
                    } else {
                        theme::TEXT
                    },
                ))
                .wrap(Wrap { trim: false }),
            columns[1],
        );
    } else if app.composer_active() {
        let context = app
            .session()
            .map(|s| {
                format!(
                    "{} · {} · step {}\n\n{}",
                    s.workflow,
                    s.status,
                    s.current_step,
                    if app.compose.mode == compose::Mode::Conversation {
                        "The manager answers or routes your original instruction. Restart previews require confirmation."
                    } else if app.compose.mode == compose::Mode::Manager {
                        "Your question goes to the read-only manager."
                    } else if app.compose.mode == compose::Mode::Command {
                        "Review a Greenlight slash command before running it."
                    } else {
                        "Your instructions target this workflow step."
                    }
                )
            })
            .unwrap_or_else(|| "Choose a session before sending a message.".into());
        frame.render_widget(
            Paragraph::new(context)
                .style(Style::default().fg(theme::MUTED))
                .wrap(Wrap { trim: false }),
            columns[1],
        );
    } else if app.attention_visible() {
        attention::draw(frame, app, columns[1], app.attention.as_ref());
    } else if app.edits.pending.is_some() || app.view == View::Edits {
        edits::draw(frame, app, columns[1]);
    } else if app.result_open && app.view == View::Conversation {
        result::draw(frame, app, columns[1]);
    } else if app.view == View::Artifacts {
        let panes = Layout::horizontal(if app.focus == Focus::Detail || app.inspector_open {
            if columns[1].width >= 100 {
                [Constraint::Percentage(38), Constraint::Percentage(62)]
            } else {
                [Constraint::Length(0), Constraint::Min(1)]
            }
        } else {
            [Constraint::Min(1), Constraint::Length(0)]
        })
        .split(columns[1]);
        let heading = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(panes[0]);
        frame.render_widget(
            Paragraph::new(format!("Artifacts · {} saved", app.artifacts.len()))
                .style(Style::default().add_modifier(Modifier::BOLD)),
            heading[0],
        );
        let list_area = heading[1];
        let name_width = list_area
            .width
            .saturating_sub(if list_area.width >= 60 { 31 } else { 3 })
            as usize;
        let items = app.artifacts.iter().map(|artifact| {
            let name = fit(&artifact.name, name_width);
            let padding = " ".repeat(
                name_width.saturating_sub(unicode_width::UnicodeWidthStr::width(name.as_str())),
            );
            let mut spans = vec![Span::styled(
                format!("{name}{padding}"),
                Style::default().fg(theme::TEXT),
            )];
            if list_area.width >= 60 {
                spans.push(Span::styled(
                    format!(
                        "  {:16} {:>9}",
                        fit(&artifact.step, 16),
                        artifact
                            .bytes
                            .map(|n| format!("{n} B"))
                            .unwrap_or_else(|| "? B".into())
                    ),
                    Style::default().fg(theme::MUTED),
                ));
            }
            ListItem::new(Line::from(spans))
        });
        let artifacts = List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(Style::default().bg(theme::PANEL));
        let mut state = ListState::default();
        if !app.artifacts.is_empty() {
            state.select(Some(app.artifact_index));
        }
        frame.render_stateful_widget(artifacts, list_area, &mut state);
        if app.artifacts.is_empty() {
            frame.render_widget(
                Paragraph::new("No artifacts yet.\nSaved workflow documents appear here.")
                    .style(Style::default().fg(theme::MUTED))
                    .wrap(Wrap { trim: false }),
                list_area,
            );
        }
        for index in state.offset()..app.artifacts.len() {
            let y = list_area.y + (index - state.offset()) as u16;
            if y >= list_area.bottom() {
                break;
            }
            app.shell.hit(
                ratatui::layout::Rect::new(list_area.x, y, list_area.width, 1),
                shell::Action::Artifact(app.artifacts[index].event_id),
            );
        }

        let title = if app.render_markdown {
            " Document · Markdown "
        } else {
            " Document · Raw text "
        };
        if let Some(error) = &app.artifact_error {
            draw_scrolled(
                frame,
                app,
                Paragraph::new(error.as_str())
                    .style(Style::default().fg(theme::ERROR))
                    .wrap(Wrap { trim: false }),
                panes[1],
            );
        } else if let Some(artifact) = app.artifact() {
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
                Block::default()
                    .style(Style::default().bg(theme::PANEL))
                    .padding(ratatui::widgets::Padding::uniform(1)),
                panes[1],
            );
        } else {
            draw_scrolled(frame,app,Paragraph::new("No artifacts on this branch yet.\n\nArtifact actions and put-artifact register documents for later steps and human review.").wrap(Wrap{trim:false}).block(border(title,app.focus==Focus::Detail)),panes[1]);
        }
    } else {
        let live = app.workflow_live_output();
        if let Some(live) = live.filter(|_| app.shell.preview && !app.blocker_card) {
            let panels = if app.shell.expanded {
                Layout::horizontal([Constraint::Length(0), Constraint::Min(1)]).split(columns[1])
            } else if columns[1].width >= 90 {
                Layout::horizontal([Constraint::Percentage(33), Constraint::Min(1)])
                    .split(columns[1])
            } else {
                Layout::vertical([
                    Constraint::Length((columns[1].height / 3).clamp(3, 7)),
                    Constraint::Min(1),
                ])
                .split(columns[1])
            };
            activity::draw_content(
                frame,
                app,
                workspace,
                panels[0],
                activity::Pane::CompactActivity,
            );
            if app.inspector_open || app.focus == Focus::Detail {
                activity::draw_content(frame, app, workspace, panels[1], activity::Pane::Detail);
            } else {
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
                    .style(Style::default())
                    .block(
                        border("", false)
                            .title(Line::styled(
                                " Live model output · unvalidated preview ",
                                Style::default().fg(theme::TEXT),
                            ))
                            .title_bottom(Line::styled(
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
                                Style::default().fg(theme::MUTED),
                            )),
                    );
                // Paragraph::line_count includes the block's top/bottom borders.
                let offset = paragraph
                    .line_count(panels[1].width.saturating_sub(2))
                    .saturating_sub(panels[1].height as usize)
                    .min(u16::MAX as usize) as u16;
                frame.render_widget(paragraph.scroll((offset, 0)), panels[1]);
            }
        } else {
            activity::draw(frame, app, workspace, columns[1]);
        }
    }

    compose::draw_composer(frame, app, outer[2]);

    let actions = app.footer_actions();
    let count = actions.len().max(1) as u16;
    let all_fit = actions
        .iter()
        .map(|a| a.label().len() as u16 + 4)
        .sum::<u16>()
        + count.saturating_sub(1)
        <= outer[3].width;
    let mut x = outer[3].x;
    for (index, action) in actions.into_iter().enumerate() {
        let preferred = action.label().len() as u16 + 4;
        let remaining = outer[3].right().saturating_sub(x);
        let remaining_count = count.saturating_sub(index as u16);
        let width = preferred
            .min(if all_fit || action == shell::Action::Overview {
                remaining
            } else {
                remaining / remaining_count
            })
            .max(1)
            .min(remaining);
        shell::button(
            frame,
            app,
            ratatui::layout::Rect::new(x, outer[3].y, width, 3),
            action,
            app.shell.actions_focus && app.shell.action_index == index,
        );
        x += width + 1;
    }
    frame.render_widget(
        Paragraph::new(inline_display(&app.notice)).style(Style::default().fg(theme::MUTED)),
        ratatui::layout::Rect::new(outer[3].x, outer[3].y + 3, outer[3].width, 1),
    );
}

fn input_panel(
    frame: &mut Frame,
    app: &App,
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
    editor.draw(
        frame,
        text,
        "Type here…",
        panels[1],
        !app.help && !app.shell.actions_focus,
    );
}

/// Fit terminal-cell text without splitting Unicode graphemes.
pub(super) fn fit(text: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let text = inline_display(text);
    if UnicodeWidthStr::width(text.as_str()) <= width {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let cells = UnicodeWidthStr::width(grapheme);
        if used + cells >= width {
            break;
        }
        out.push_str(grapheme);
        used += cells;
    }
    out.push('…');
    out
}
