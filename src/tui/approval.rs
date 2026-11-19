//! Explicit approval decisions and their display choices.
use super::*;

impl App {
    pub(super) fn approval_visible(&self) -> bool {
        self.approval.is_some() && !self.approval_hidden
    }
    pub(super) fn permission_popup_visible(&self) -> bool {
        self.approval_visible()
            && self
                .approval
                .as_ref()
                .is_some_and(|p| p.mode != ApprovalMode::Human)
    }
    pub(super) fn provider_retry_pending(&self) -> bool {
        self.approval
            .as_ref()
            .is_some_and(|p| p.mode == ApprovalMode::ProviderRetry)
    }
    pub(super) fn dismiss_permission(&mut self) {
        if !self.permission_popup_visible() {
            return;
        }
        self.approval_hidden = true;
        self.detail_scroll = self.approval_return_scroll;
        self.shell.actions_focus = false;
        self.notice = if self.provider_retry_pending() {
            "Provider request paused. [a] Review retry; no new request sent."
        } else {
            "Permission pending — agent is waiting. [a] Review permission; no decision sent."
        }
        .into();
    }
    pub(super) fn reopen_permission(&mut self) {
        if self.approval.is_none() || !self.approval_hidden {
            return;
        }
        if let Some(id) = self.runner.as_ref().and_then(|r| r.session_id.clone()) {
            if let Some(index) = self.sessions.iter().position(|s| s.id == id) {
                self.session_index = index;
            }
            self.requested_session = Some(id);
        }
        self.approval_return_scroll = self.detail_scroll;
        self.detail_scroll = 0;
        self.approval_hidden = false;
        self.shell.more = false;
        self.help = false;
        self.shell.actions_focus = false;
    }

    pub(super) fn deliver_approval(&mut self, choice: char) -> Result<()> {
        if !self.approval_visible() {
            bail!("Open the permission popup before deciding");
        }
        let request = self.approval.as_ref().context("no pending approval")?;
        if !request.mode.accepts(choice) {
            bail!("approval option {choice} is unavailable for this request");
        }
        let retry = request.mode == ApprovalMode::ProviderRetry;
        let human = request.mode == ApprovalMode::Human;
        let review_kind = request.review.as_ref().map(|r| r.kind);
        let confirmation = request.mode == ApprovalMode::ConfirmPersistent;
        let runner = self.runner.as_ref().context("approval worker missing")?;
        runner
            .answers
            .send(choice.to_string())
            .context("approval worker disconnected")?;
        self.approval = None;
        self.approval_hidden = false;
        self.detail_scroll = self.approval_return_scroll;
        self.notice = match choice {
            'y' if retry => "Retry requested for the same model messages; waiting for any provider cooldown.".into(),
            'n' if retry => "Stopped the provider retry; recorded work remains available.".into(),
            'y' if human && review_kind == Some(crate::tools::HumanReviewKind::Checkpoint) => "Sent checkpoint approval; awaiting the engine’s input recheck and recorded outcome.".into(),
            'y' if human => "Sent human step-result decision; awaiting its recorded outcome.".into(),
            'n' if human => "Sent decline; the workflow follows its configured handling.".into(),
            'y' if confirmation => "Confirmed persistent grant; awaiting engine result.".into(),
            'y' => "Sent approval to the engine; awaiting its recorded outcome.".into(),
            'a' => "Sent process-wide approval to the engine.".into(),
            'p' => "Requested project-wide grant; awaiting engine result.".into(),
            'g' => "Requested global grant; awaiting engine result.".into(),
            'n' => "Denied this request; the agent may attempt another action.".into(),
            _ => unreachable!("choices validated above"),
        };
        Ok(())
    }
}

pub(super) fn approval_line(key: char, description: &'static str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  [{key}] "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(description),
    ])
}

pub(super) fn approval_options(mode: ApprovalMode) -> Vec<Line<'static>> {
    let mut options = vec![approval_line(
        'y',
        if mode == ApprovalMode::ProviderRetry {
            "Retry request"
        } else if mode == ApprovalMode::ConfirmPersistent {
            "Save grant and allow"
        } else if mode == ApprovalMode::Human {
            "Approve checkpoint"
        } else {
            "Allow once"
        },
        theme::FOCUS,
    )];
    if matches!(mode, ApprovalMode::Tool | ApprovalMode::Persistent) {
        options.push(approval_line('a', "Allow for this process", Color::Cyan));
    }
    if mode == ApprovalMode::Persistent {
        options.push(approval_line('p', "Review project grant", Color::Yellow));
        options.push(approval_line('g', "Review global grant", Color::Yellow));
    }
    options.push(approval_line(
        'n',
        if mode == ApprovalMode::ProviderRetry {
            "Stop for now"
        } else {
            "Deny"
        },
        Color::Red,
    ));
    options.push(Line::styled(
        "  Esc closes without deciding; the agent keeps waiting.",
        Style::default().fg(Color::DarkGray),
    ));
    options
}

fn visible_command(command: &str) -> String {
    command.chars().map(|c| {
        if (c.is_control() && c != '\n') || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}') {
            c.escape_default().to_string()
        } else {c.to_string()}
    }).collect()
}

/// Typed command metadata controls emphasis only; decision keys still use ApprovalMode.
fn approval_text(prompt: &ApprovalPrompt) -> Text<'static> {
    let description = prompt
        .content
        .rsplit_once("\n[y]")
        .map_or(prompt.content.as_str(), |(text, _)| text);
    let mut lines = Vec::new();
    let mut details = description;
    if let Some(command) = &prompt.command {
        lines.push(Line::styled(
            "Command to execute",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        let command_style = Style::default()
            .fg(Color::LightCyan)
            .bg(Color::Rgb(20, 24, 36))
            .add_modifier(Modifier::BOLD);
        lines.extend(
            visible_command(command)
                .split('\n')
                .map(|line| Line::styled(line.to_owned(), command_style)),
        );
        lines.push(Line::default());
        let prefix = format!("[approval required: shell]\n{command}\n");
        if let Some(rest) = description.strip_prefix(&prefix) {
            details = rest;
        }
    }
    for line in safe_display(details).split('\n') {
        let style = if line.starts_with("Scope:") || line.starts_with("Applies to:") {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if line.starts_with("Shell is isolated:")
            || line.starts_with("Future matching requests")
        {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        };
        lines.push(Line::styled(line.to_owned(), style));
    }
    Text::from(lines)
}

pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    prompt: &ApprovalPrompt,
    area: ratatui::layout::Rect,
) {
    if prompt.mode == ApprovalMode::Human {
        if let Some(review) = &prompt.review {
            checkpoint::draw(frame, app, review, area);
            return;
        }
    }
    let choices_height = match prompt.mode {
        ApprovalMode::Human | ApprovalMode::ConfirmPersistent | ApprovalMode::ProviderRetry => 5,
        ApprovalMode::Tool => 6,
        ApprovalMode::Persistent => 8,
    };
    let approval_areas =
        Layout::vertical([Constraint::Min(3), Constraint::Length(choices_height)]).split(area);
    let title = if prompt.mode == ApprovalMode::ProviderRetry {
        " Provider status "
    } else if prompt.mode == ApprovalMode::ConfirmPersistent {
        " Confirm persistent grant "
    } else if prompt.mode == ApprovalMode::Human {
        " Human approval required "
    } else {
        " Tool permission required "
    };
    draw_scrolled(
        frame,
        app,
        Paragraph::new(approval_text(prompt))
            .wrap(Wrap { trim: false })
            .block(
                border("", true).title(format!(
                    "{} · {}",
                    title.trim(),
                    super::view::fit(
                        app.session()
                            .map(|s| s.goal.as_str())
                            .unwrap_or("Current request"),
                        40
                    )
                )),
            ),
        approval_areas[0],
    );
    let keys = ['y', 'a', 'p', 'g', 'n']
        .into_iter()
        .filter(|key| prompt.mode.accepts(*key))
        .collect::<Vec<_>>();
    for (index, key) in keys.into_iter().enumerate() {
        let y = approval_areas[1].y + 1 + index as u16;
        if y < approval_areas[1].bottom().saturating_sub(1) {
            app.shell.hit(
                ratatui::layout::Rect::new(
                    approval_areas[1].x + 1,
                    y,
                    approval_areas[1].width.saturating_sub(2),
                    1,
                ),
                shell::Action::Decision(key),
            );
        }
    }
    frame.render_widget(
        Paragraph::new(approval_options(prompt.mode))
            .block(border(" Press a key to decide — no Enter needed ", true)),
        approval_areas[1],
    );
}

/// Tool decisions are a modal layer; underlying hit targets must never receive clicks.
pub(super) fn draw_popup(frame: &mut Frame, app: &App) {
    let Some(prompt) = app
        .approval
        .as_ref()
        .filter(|_| app.permission_popup_visible())
    else {
        return;
    };
    app.shell.hits.borrow_mut().clear();
    let area = frame.area();
    if area.width < 40 || area.height < 20 {
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(Paragraph::new("Resize to at least 40×20 to decide.\nPermission pending. Esc closes; agent keeps waiting.").wrap(Wrap{trim:false}),area);
        return;
    }
    let width = area.width.saturating_sub(4).min(100);
    let height = area.height.saturating_sub(2).min(26);
    let popup = ratatui::layout::Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(ratatui::widgets::Clear, popup);
    frame.render_widget(
        Block::default()
            .style(Style::default().bg(theme::PANEL))
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(theme::ATTENTION))
            .title(if prompt.mode == ApprovalMode::ProviderRetry {
                " Provider request paused · Esc closes; request waits "
            } else {
                " Permission required · Esc closes; agent waits "
            }),
        popup,
    );
    let inner =
        ratatui::layout::Rect::new(popup.x + 1, popup.y + 1, popup.width - 2, popup.height - 2);
    draw(frame, app, prompt, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_multiline_command_is_emphasized_separately_from_scope() {
        use ratatui::{backend::TestBackend, Terminal};
        let command = "cat something.txt | ./harmful_script.sh\nprintf 'Scope: not a grant'";
        let prompt=ApprovalPrompt { review: None,
            command:Some(command.into()),mode:ApprovalMode::Persistent,
            content:format!("[approval required: shell]\n{command}\nShell is isolated: read-only project, no network, private HOME.\nScope: this exact command (including arguments and operators).\n[y] once / [n] deny"),
        };
        let app = App::new(None);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| draw(f, &app, &prompt, f.area())).unwrap();
        let buffer = terminal.backend().buffer();
        let cyan = buffer
            .content
            .iter()
            .filter(|c| c.fg == Color::LightCyan && c.modifier.contains(Modifier::BOLD))
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(cyan.contains("cat something.txt | ./harmful_script.sh"));
        assert!(cyan.contains("printf 'Scope: not a grant'"));
        let yellow = buffer
            .content
            .iter()
            .filter(|c| c.fg == Color::Yellow)
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(yellow.contains("Scope: this exact command"));
        assert!(prompt.mode.accepts('p'));
        assert!(!ApprovalMode::Human.accepts('p'));
    }
    #[test]
    fn command_rendering_uses_typed_metadata_and_exposes_terminal_controls() {
        let prompt = ApprovalPrompt {
            review: None,
            command: Some("printf '\u{1b}[2J'".into()),
            mode: ApprovalMode::Tool,
            content: "Tool says [approval required: shell]\nnot-the-command".into(),
        };
        let rendered = approval_text(&prompt);
        assert_eq!(rendered.lines[0].to_string(), "Command to execute");
        assert_eq!(rendered.lines[1].to_string(), "printf '\\u{1b}[2J'");
        assert!(!rendered.to_string().contains('\u{1b}'));
    }
}
