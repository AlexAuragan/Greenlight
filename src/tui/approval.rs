//! Explicit approval decisions and their display choices.
use super::*;

impl App {
    pub(super) fn deliver_approval(&mut self, choice: char) -> Result<()> {
        let request = self.approval.as_ref().context("no pending approval")?;
        if !request.mode.accepts(choice) {
            bail!("approval option {choice} is unavailable for this request");
        }
        let confirmation = request.mode == ApprovalMode::ConfirmPersistent;
        let runner = self.runner.as_ref().context("approval worker missing")?;
        runner
            .answers
            .send(choice.to_string())
            .context("approval worker disconnected")?;
        self.approval = None;
        self.notice = match choice {
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
        if mode == ApprovalMode::ConfirmPersistent {
            "Save grant and allow"
        } else if mode == ApprovalMode::Human {
            "Approve checkpoint"
        } else {
            "Allow once"
        },
        Color::Green,
    )];
    if matches!(mode, ApprovalMode::Tool | ApprovalMode::Persistent) {
        options.push(approval_line('a', "Allow for this process", Color::Cyan));
    }
    if mode == ApprovalMode::Persistent {
        options.push(approval_line('p', "Review project grant", Color::Yellow));
        options.push(approval_line('g', "Review global grant", Color::Yellow));
    }
    options.push(approval_line('n', "Deny (Esc also denies)", Color::Red));
    options.push(Line::styled(
        "  Each key sends immediately. Enter has no effect.",
        Style::default().fg(Color::DarkGray),
    ));
    options
}

pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    prompt: &ApprovalPrompt,
    area: ratatui::layout::Rect,
) {
    let choices_height = match prompt.mode {
        ApprovalMode::Human | ApprovalMode::ConfirmPersistent => 5,
        ApprovalMode::Tool => 6,
        ApprovalMode::Persistent => 8,
    };
    let approval_areas =
        Layout::vertical([Constraint::Min(3), Constraint::Length(choices_height)]).split(area);
    let title = if prompt.mode == ApprovalMode::ConfirmPersistent {
        " Confirm persistent grant "
    } else if prompt.mode == ApprovalMode::Human {
        " Human approval required "
    } else {
        " Tool permission required "
    };
    draw_scrolled(
        frame,
        app,
        Paragraph::new(
            prompt
                .content
                .rsplit_once("\n[y]")
                .map_or(prompt.content.as_str(), |(description, _)| description)
                .to_string(),
        )
        .wrap(Wrap { trim: false })
        .block(border(title, true)),
        approval_areas[0],
    );
    frame.render_widget(
        Paragraph::new(approval_options(prompt.mode))
            .block(border(" Press a key to decide — no Enter needed ", true)),
        approval_areas[1],
    );
}
