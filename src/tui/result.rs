//! Completion is control flow, not evidence that the user's objective was achieved.
use super::*;
pub(super) fn summary(
    store: &Store,
    session: &Session,
    history: &[(i64, String, String, String)],
) -> String {
    let end = history
        .iter()
        .rev()
        .find(|e| e.2 == "session.completed")
        .map(|e| e.0)
        .unwrap_or(i64::MAX);
    let start = history
        .iter()
        .rev()
        .find(|e| e.0 <= end && e.1 == session.current_step && e.2 == "step.started")
        .map(|e| e.0)
        .unwrap_or(0);
    let changed = history
        .iter()
        .filter(|e| e.2 == "file.changed")
        .map(|e| e.3.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut out = format!(
        "Workflow ended · {}\nSource edits: {} file(s) recorded · 3 opens diffs/revert.\n",
        session.workflow,
        changed.len()
    );
    let in_final = |e: &&(i64, String, String, String)| {
        e.0 > start && e.0 <= end && e.1 == session.current_step
    };
    let checks = if history
        .iter()
        .filter(in_final)
        .any(|e| e.2 == "verification.none")
        && !history.iter().filter(in_final).any(|e| {
            matches!(
                e.2.as_str(),
                "verification.command"
                    | "verification.agent"
                    | "verification.human"
                    | "verification.workspace"
            )
        }) {
        "Verification: no checks required (policy accepted, not measured validation)."
    } else if history
        .iter()
        .filter(in_final)
        .any(|e| e.2 == "verification.passed")
    {
        "Verification: configured policy passed; only its checks were validated."
    } else {
        "Verification: no recorded aggregate verdict."
    };
    out.push_str(&format!("{checks}\n"));
    if history
        .iter()
        .any(|e| e.0 > end && e.2 == "workspace.restored")
    {
        out.push_str("Workspace restored after this result: reports/checks describe the historical run, not the restored files.\n");
    }
    if let Some((id, _, _, text)) = history
        .iter()
        .filter(in_final)
        .rev()
        .find(|e| e.2 == "step.completed")
    {
        out.push_str(&format!(
            "\nAgent-reported conclusion · event #{id}:\n{text}\n"
        ));
    }
    out.push_str("\nWorkflow completion alone does not establish that the goal was achieved.\n");
    let required = history
        .iter()
        .rev()
        .find(|e| e.0 <= start && e.1 == session.current_step && e.2 == "step.definition")
        .and_then(|e| serde_yaml::from_str::<crate::config::Step>(&e.3).ok())
        .map(|step| step.outputs.required)
        .unwrap_or_default();
    if let Some((id, _, _, name)) = history.iter().filter(in_final).rev().find(|e| {
        e.2 == "artifact.created"
            && (required.is_empty() && !crate::verification_workspace::is_generated_diff(&e.3)
                || required.contains(&e.3))
    }) {
        match store.artifact_version(session,name,*id) {
            Ok(report)=>{
                let excerpt=report.content.chars().take(10000).collect::<String>();
                out.push_str(&format!("\nFinal report · {} · event #{}\nModel-authored assessment; inspect its evidence.\n\n{}{}\n",report.name,report.event_id,excerpt,if excerpt.len()<report.content.len(){"\n[Preview truncated; open full report in 2 Artifacts / version history.]"}else{""}));
            }
            Err(error)=>out.push_str(&format!("\nFinal report · {name} · event #{id}\nRecorded report body unavailable: {error:#}\n")),
        }
    } else {
        out.push_str(
            "\nNo final-step report was registered. Review the step summaries and artifacts.\n",
        );
    }
    out.push_str(&format!(
        "\nGoal: {}\n\nRecorded step summaries (including earlier attempts):\n",
        session.goal
    ));
    for (id, step, kind, text) in history {
        if kind == "step.completed" && *id <= end {
            out.push_str(&format!(
                "\n{step} · event #{id} · agent reported:\n{text}\n"
            ));
        }
    }
    out.push_str("\n2: artifacts · 3: source edits/revert · H/Esc: activity · s: reopen result\n");
    out
}

/// Evidence-backed fields for the overview; model prose is never an outcome classifier.
#[derive(Default)]
pub(super) struct Overview {
    pub conclusion: String,
    pub verification: &'static str,
    pub checked: bool,
    pub edits: usize,
    pub historical: bool,
    pub report: Option<(String, i64)>,
}
pub(super) fn overview(session: &Session, history: &[(i64, String, String, String)]) -> Overview {
    let end = history
        .iter()
        .rev()
        .find(|e| e.2 == "session.completed")
        .map(|e| e.0)
        .unwrap_or(i64::MAX);
    let start = history
        .iter()
        .rev()
        .find(|e| e.0 <= end && e.1 == session.current_step && e.2 == "step.started")
        .map(|e| e.0)
        .unwrap_or(0);
    let final_events = history
        .iter()
        .filter(|e| e.0 > start && e.0 <= end && e.1 == session.current_step)
        .collect::<Vec<_>>();
    let none = final_events.iter().any(|e| e.2 == "verification.none")
        && !final_events.iter().any(|e| {
            matches!(
                e.2.as_str(),
                "verification.command"
                    | "verification.agent"
                    | "verification.human"
                    | "verification.workspace"
            )
        });
    let checked = !none && final_events.iter().any(|e| e.2 == "verification.passed");
    let required = history
        .iter()
        .rev()
        .find(|e| e.0 <= start && e.1 == session.current_step && e.2 == "step.definition")
        .and_then(|e| serde_yaml::from_str::<crate::config::Step>(&e.3).ok())
        .map(|s| s.outputs.required)
        .unwrap_or_default();
    Overview {
        conclusion: final_events
            .iter()
            .rev()
            .find(|e| e.2 == "step.completed")
            .map(|e| e.3.clone())
            .unwrap_or_else(|| "No final-step conclusion recorded.".into()),
        verification: if none {
            "No checks required by the final-step policy."
        } else if checked {
            "Configured checks passed · final step only"
        } else {
            "No recorded aggregate verification verdict."
        },
        checked,
        edits: history
            .iter()
            .filter(|e| e.2 == "file.changed")
            .map(|e| &e.3)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        historical: history
            .iter()
            .any(|e| e.0 > end && e.2 == "workspace.restored"),
        report: final_events
            .iter()
            .rev()
            .find(|e| {
                e.2 == "artifact.created"
                    && (required.is_empty()
                        && !crate::verification_workspace::is_generated_diff(&e.3)
                        || required.contains(&e.3))
            })
            .map(|e| (e.3.clone(), e.0)),
    }
}

pub(super) fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(result) = &app.result_overview else {
        return;
    };
    let width = area.width.saturating_sub(4) as usize;
    if width == 0 {
        return;
    }
    let mut lines = vec![
        Line::styled(
            if result.historical {
                "Historical result · workspace restored"
            } else {
                "Result"
            },
            Style::default()
                .fg(if result.historical {
                    theme::ATTENTION
                } else {
                    theme::TEXT
                })
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            if result.historical {
                "Reports/checks describe the earlier workspace, not restored files."
            } else {
                "Workflow finished. Review the conclusion and evidence."
            },
            Style::default().fg(if result.historical {
                theme::ATTENTION
            } else {
                theme::MUTED
            }),
        ),
        Line::default(),
        Line::styled(
            " Agent-reported conclusion",
            Style::default().fg(theme::MUTED).bg(theme::PANEL),
        ),
    ];
    // Explicit excerpt; never insert the report body or infer a success/failure badge.
    let words = inline_display(&result.conclusion);
    let mut wrapped = vec![String::new()];
    for word in words.split_whitespace() {
        let current = wrapped.last_mut().unwrap();
        if !current.is_empty()
            && unicode_width::UnicodeWidthStr::width(current.as_str())
                + 1
                + unicode_width::UnicodeWidthStr::width(word)
                > width
        {
            wrapped.push(String::new());
        }
        let current = wrapped.last_mut().unwrap();
        if !current.is_empty() {
            current.push(' ')
        }
        current.push_str(word);
    }
    let clipped = wrapped.len() > 3
        || wrapped
            .iter()
            .any(|s| unicode_width::UnicodeWidthStr::width(s.as_str()) > width);
    for line in wrapped.iter().take(3) {
        lines.push(Line::styled(
            format!(" {}", super::view::fit(line, width)),
            Style::default().fg(theme::TEXT).bg(theme::PANEL),
        ));
    }
    if clipped {
        lines.push(Line::styled(
            " Excerpt · More → Full result",
            Style::default().fg(theme::MUTED).bg(theme::PANEL),
        ));
    }
    let card_width = area.width.saturating_sub(2) as usize;
    for line in lines.iter_mut().skip(3) {
        let used = unicode_width::UnicodeWidthStr::width(line.to_string().as_str());
        line.spans
            .push(Span::raw(" ".repeat(card_width.saturating_sub(used))));
    }
    lines.extend([
        Line::default(),
        Line::styled(
            "Recorded verification",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            result.verification,
            Style::default().fg(if result.checked {
                theme::GOOD
            } else {
                theme::ATTENTION
            }),
        ),
        Line::styled(
            if result.checked {
                "Only the configured checks were validated."
            } else {
                "Policy acceptance is not measured validation."
            },
            Style::default().fg(theme::MUTED),
        ),
        Line::default(),
        Line::styled(
            "Recorded native edits",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(format!(
            "{} file(s) · Review edits for diffs and guarded revert",
            result.edits
        )),
        Line::styled(
            "Shell/manual changes are outside this journal.",
            Style::default().fg(theme::MUTED),
        ),
    ]);
    lines.push(Line::default());
    lines.push(Line::styled(
        result
            .report
            .as_ref()
            .map(|(name, _)| format!("Final report: {name} · exact saved version"))
            .unwrap_or_else(|| "No final-step report recorded. Review available evidence.".into()),
        Style::default().fg(theme::MUTED),
    ));
    draw_scrolled(
        frame,
        app,
        Paragraph::new(lines)
            .style(Style::default().fg(theme::TEXT))
            .wrap(Wrap { trim: false })
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1))),
        area,
    );
}
