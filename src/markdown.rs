//! Lightweight artifact Markdown presentation (source remains unchanged).
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

fn flush(spans: &mut Vec<Span<'static>>, text: &mut String, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(std::mem::take(text), style));
    }
}

// Render common inline constructs. Cap recursive nesting for untrusted artifacts.
fn inline(input: &str, style: Style, depth: usize) -> Vec<Span<'static>> {
    if depth >= 8 {
        return vec![Span::styled(input.to_owned(), style)];
    }
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut pos = 0;
    while pos < input.len() {
        let rest = &input[pos..];
        if let Some(ch) = rest.strip_prefix('\\')
            .and_then(|escaped| escaped.chars().next())
            .filter(|ch| "\\*_`[]".contains(*ch))
        {
            plain.push(ch);
            pos += 1 + ch.len_utf8();
            continue;
        }
        if let Some((after, end)) = rest.strip_prefix('`')
            .and_then(|after| after.find('`').map(|end| (after, end)))
        {
            flush(&mut spans, &mut plain, style);
            spans.push(Span::styled(after[..end].to_owned(), Style::default().fg(Color::Yellow)));
            pos += end + 2;
            continue;
        }
        let marker = if rest.starts_with("**") { Some("**") }
            else if rest.starts_with("__") { Some("__") }
            else if rest.starts_with('*') { Some("*") }
            else if rest.starts_with('_') { Some("_") }
            else { None };
        if let Some(marker) = marker {
            let identifier = marker.starts_with('_') && pos > 0
                && input[..pos].chars().next_back().is_some_and(char::is_alphanumeric);
            if !identifier {
                let after = &rest[marker.len()..];
                if let Some(end) = after.find(marker) {
                    let inside = &after[..end];
                    if !inside.is_empty() && inside.trim() == inside {
                        flush(&mut spans, &mut plain, style);
                        let modifier = if marker.len() == 2 { Modifier::BOLD } else { Modifier::ITALIC };
                        spans.extend(inline(inside, style.add_modifier(modifier), depth + 1));
                        pos += marker.len() * 2 + end;
                        continue;
                    }
                }
            }
        }
        let ch = rest.chars().next().expect("slice is nonempty");
        plain.push(ch);
        pos += ch.len_utf8();
    }
    flush(&mut spans, &mut plain, style);
    spans
}

fn cells(line: &str) -> Option<Vec<&str>> {
    let line = line.trim();
    if !line.contains('|') { return None; }
    let parts = line.trim_start_matches('|').trim_end_matches('|')
        .split('|').map(str::trim).collect::<Vec<_>>();
    (parts.len() >= 2).then_some(parts)
}

fn table_at<'a>(lines: &[&'a str], start: usize) -> Option<(Vec<Vec<&'a str>>, usize)> {
    let head = cells(lines.get(start)?)?;
    let rule = cells(lines.get(start + 1)?)?;
    if head.len() != rule.len() || !rule.iter().all(|cell| {
        let dashes = cell.trim_matches(':');
        dashes.len() >= 3 && dashes.bytes().all(|b| b == b'-')
    }) { return None; }
    let mut rows = vec![head];
    let mut end = start + 2;
    while let Some(row) = lines.get(end).and_then(|line| cells(line)) {
        if row.len() != rows[0].len() { break; }
        rows.push(row);
        end += 1;
    }
    Some((rows, end))
}

fn display_width(text: &str) -> usize {
    inline(text, Style::default(), 0).iter()
        .map(|span| span.content.chars().count()).sum()
}

fn add_table(output: &mut Vec<Line<'static>>, rows: &[Vec<&str>]) {
    let widths = (0..rows[0].len())
        .map(|col| rows.iter().map(|row| display_width(row[col])).max().unwrap_or(0))
        .collect::<Vec<_>>();
    for (index, row) in rows.iter().enumerate() {
        let style = if index == 0 {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let mut spans = vec![Span::styled("│ ", Style::default().fg(Color::DarkGray))];
        for (col, cell) in row.iter().enumerate() {
            spans.extend(inline(cell, style, 0));
            spans.push(Span::raw(" ".repeat(widths[col].saturating_sub(display_width(cell)))));
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
        output.push(Line::from(spans));
        if index == 0 {
            output.push(Line::styled(
                "─".repeat((widths.iter().sum::<usize>() + widths.len() * 3).min(100)),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let n = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&n) { return None; }
    line.get(n..)?.strip_prefix(' ').map(|text| (n, text))
}

fn ordered(line: &str) -> Option<(&str, &str)> {
    let n = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 || line.as_bytes().get(n) != Some(&b'.')
        || line.as_bytes().get(n + 1) != Some(&b' ') { return None; }
    Some((&line[..n + 1], &line[n + 2..]))
}

/// Format headings, emphasis, code, lists, quotes and simple pipe tables.
/// The raw toggle preserves full-fidelity access to unsupported Markdown.
pub fn render(source: &str) -> Text<'static> {
    let lines = source.split('\n').collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;
    let mut fence: Option<&str> = None;
    while index < lines.len() {
        let line = lines[index].trim_end_matches('\r');
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) { fence = None; }
            else { result.push(Line::styled(format!("  {line}"), Style::default().fg(Color::LightGreen))); }
            index += 1;
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = if trimmed.starts_with("```") { "```" } else { "~~~" };
            let lang = trimmed[3..].trim();
            result.push(Line::styled(
                if lang.is_empty() { "  Code".to_owned() } else { format!("  Code · {lang}") },
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ));
            fence = Some(marker);
            index += 1;
            continue;
        }
        if let Some((rows, end)) = table_at(&lines, index) {
            add_table(&mut result, &rows);
            index = end;
            continue;
        }
        if let Some((level, content)) = heading(trimmed) {
            let style = Style::default()
                .fg(if level == 1 { Color::Cyan } else { Color::Blue })
                .add_modifier(Modifier::BOLD);
            if !result.is_empty() { result.push(Line::default()); }
            let mut spans = vec![Span::styled("▍ ", style)];
            spans.extend(inline(content, style, 0));
            result.push(Line::from(spans));
        } else if let Some(content) = trimmed.strip_prefix("> ") {
            let style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
            let mut spans = vec![Span::styled("│ ", style)];
            spans.extend(inline(content, style, 0));
            result.push(Line::from(spans));
        } else if let Some(content) = trimmed.strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let mut spans = vec![Span::styled(
                format!("{}• ", " ".repeat(line.len() - trimmed.len())),
                Style::default().fg(Color::Cyan),
            )];
            spans.extend(inline(content, Style::default(), 0));
            result.push(Line::from(spans));
        } else if let Some((number, content)) = ordered(trimmed) {
            let mut spans = vec![Span::styled(
                format!("{}{} ", " ".repeat(line.len() - trimmed.len()), number),
                Style::default().fg(Color::Cyan),
            )];
            spans.extend(inline(content, Style::default(), 0));
            result.push(Line::from(spans));
        } else if matches!(trimmed, "---" | "***" | "___") {
            result.push(Line::styled("─────────────────────────", Style::default().fg(Color::DarkGray)));
        } else {
            result.push(Line::from(inline(line, Style::default(), 0)));
        }
        index += 1;
    }
    Text::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(line: &Line<'_>) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect::<Vec<_>>().concat()
    }

    #[test]
    fn headings_emphasis_and_code_are_styled() {
        let text = render("# Plan\n- **Bold** and *italic* with `code`");
        assert_eq!(shown(&text.lines[0]), "▍ Plan");
        assert_eq!(shown(&text.lines[1]), "• Bold and italic with code");
        assert!(text.lines[1].spans.iter().any(|s| s.content == "Bold"
            && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(text.lines[1].spans.iter().any(|s| s.content == "italic"
            && s.style.add_modifier.contains(Modifier::ITALIC)));
        assert!(text.lines[1].spans.iter().any(|s| s.content == "code"
            && s.style.fg == Some(Color::Yellow)));
    }

    #[test]
    fn table_has_header_and_rows() {
        let text = render("| Name | Status |\n| --- | --- |\n| Build | **ok** |");
        assert!(shown(&text.lines[0]).contains("Name"));
        assert!(shown(&text.lines[2]).contains("Build"));
        assert!(shown(&text.lines[2]).contains("ok"));
    }

    #[test]
    fn fenced_code_is_not_parsed_as_markdown() {
        let text = render("```python\n# not a heading\n**not bold**\n```");
        assert_eq!(shown(&text.lines[0]), "  Code · python");
        assert_eq!(shown(&text.lines[1]), "  # not a heading");
        assert_eq!(shown(&text.lines[2]), "  **not bold**");
    }

    #[test]
    fn escapes_identifiers_and_unicode() {
        let text = render(r"\*literal\* foo_bar_baz résumé");
        assert_eq!(shown(&text.lines[0]), "*literal* foo_bar_baz résumé");
    }

    #[test]
    fn nested_emphasis() {
        let text = render("**Bold *and italic* text**");
        assert_eq!(shown(&text.lines[0]), "Bold and italic text");
    }
}
