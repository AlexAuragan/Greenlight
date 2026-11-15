//! Lightweight artifact Markdown presentation (source remains unchanged).
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

// Bound display work independently of the workflow's configurable artifact limit.
pub(crate) const MAX_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 4096;
const MAX_LINE_BYTES: usize = 8192;
const MAX_COLUMNS: usize = 32;
const TRUNCATED: &str = "[Preview truncated at display limits. Use CLI history/artifact exports for complete recorded content.]";

/// Terminal presentation only. Keep source bytes untouched in the store.
fn preview(source: &str) -> (String, bool) {
    let mut result = String::with_capacity(source.len().min(MAX_BYTES));
    let mut lines = 1;
    let mut line_bytes = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if offset >= MAX_BYTES {
            return (result, true);
        }
        // Normalize CRLF, but make a lone carriage return visible.
        if ch == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
            continue;
        }
        let mut encoded = [0u8; 4];
        let escaped;
        let visible: &str = match ch {
            '\n' => "\n",
            '\t' => "    ",
            ch if ch.is_control()
                || matches!(ch,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') =>
            {
                escaped = format!("\\u{{{:04x}}}", ch as u32);
                &escaped
            }
            _ => ch.encode_utf8(&mut encoded),
        };
        if result.len() + visible.len() > MAX_BYTES
            || (ch != '\n' && line_bytes + visible.len() > MAX_LINE_BYTES)
            || (ch == '\n' && lines >= MAX_LINES)
        {
            return (result, true);
        }
        result.push_str(visible);
        if ch == '\n' {
            lines += 1;
            line_bytes = 0;
        } else {
            line_bytes += visible.len();
        }
    }
    (result, false)
}

/// Literal, terminal-safe preview with the same limits as Markdown.
pub fn raw(source: &str) -> Text<'static> {
    let (mut text, truncated) = preview(source);
    if truncated {
        text.push('\n');
        text.push_str(TRUNCATED);
    }
    Text::raw(text)
}

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
        if let Some(ch) = rest
            .strip_prefix('\\')
            .and_then(|escaped| escaped.chars().next())
            .filter(|ch| "\\*_`[]".contains(*ch))
        {
            plain.push(ch);
            pos += 1 + ch.len_utf8();
            continue;
        }
        if let Some((after, end)) = rest
            .strip_prefix('`')
            .and_then(|after| after.find('`').map(|end| (after, end)))
        {
            flush(&mut spans, &mut plain, style);
            spans.push(Span::styled(
                after[..end].to_owned(),
                Style::default().fg(Color::Yellow),
            ));
            pos += end + 2;
            continue;
        }
        let marker = if rest.starts_with("**") {
            Some("**")
        } else if rest.starts_with("__") {
            Some("__")
        } else if rest.starts_with('*') {
            Some("*")
        } else if rest.starts_with('_') {
            Some("_")
        } else {
            None
        };
        if let Some(marker) = marker {
            let identifier = marker.starts_with('_')
                && pos > 0
                && input[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric);
            if !identifier {
                let after = &rest[marker.len()..];
                if let Some(end) = after.find(marker) {
                    let inside = &after[..end];
                    if !inside.is_empty() && inside.trim() == inside {
                        flush(&mut spans, &mut plain, style);
                        let modifier = if marker.len() == 2 {
                            Modifier::BOLD
                        } else {
                            Modifier::ITALIC
                        };
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
    if !line.contains('|') {
        return None;
    }
    let parts = line
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    (parts.len() >= 2 && parts.len() <= MAX_COLUMNS).then_some(parts)
}

fn table_at<'a>(lines: &[&'a str], start: usize) -> Option<(Vec<Vec<&'a str>>, usize)> {
    let head = cells(lines.get(start)?)?;
    let rule = cells(lines.get(start + 1)?)?;
    if head.len() != rule.len()
        || !rule.iter().all(|cell| {
            let dashes = cell.trim_matches(':');
            dashes.len() >= 3 && dashes.bytes().all(|b| b == b'-')
        })
    {
        return None;
    }
    let mut rows = vec![head];
    let mut end = start + 2;
    while let Some(row) = lines.get(end).and_then(|line| cells(line)) {
        if row.len() != rows[0].len() {
            break;
        }
        rows.push(row);
        end += 1;
    }
    Some((rows, end))
}

fn add_table(output: &mut Vec<Line<'static>>, rows: &[Vec<&str>]) {
    // Parse each cell once, measuring with the same terminal-width rules as Ratatui.
    let rendered = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let style = if index == 0 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            row.iter()
                .map(|cell| {
                    let spans = inline(cell, style, 0);
                    let width: usize = spans.iter().map(Span::width).sum();
                    (spans, width)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // Padding cannot amplify sparse adversarial tables without bound.
    let padding_limit = 240 / rows[0].len();
    let widths = (0..rows[0].len())
        .map(|col| {
            rendered
                .iter()
                .map(|row| row[col].1)
                .max()
                .unwrap_or(0)
                .min(padding_limit)
        })
        .collect::<Vec<_>>();
    for (index, row) in rendered.into_iter().enumerate() {
        let mut spans = vec![Span::styled("│ ", Style::default().fg(Color::DarkGray))];
        for (col, (cell, width)) in row.into_iter().enumerate() {
            spans.extend(cell);
            spans.push(Span::raw(" ".repeat(widths[col].saturating_sub(width))));
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
    if !(1..=6).contains(&n) {
        return None;
    }
    line.get(n..)?.strip_prefix(' ').map(|text| (n, text))
}

fn ordered(line: &str) -> Option<(&str, &str)> {
    let n = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 || line.as_bytes().get(n) != Some(&b'.') || line.as_bytes().get(n + 1) != Some(&b' ')
    {
        return None;
    }
    Some((&line[..n + 1], &line[n + 2..]))
}

/// Format headings, emphasis, code, lists, quotes and simple pipe tables.
/// Both preview modes escape controls and bound work; source remains unchanged.
pub fn render(source: &str) -> Text<'static> {
    let (source, truncated) = preview(source);
    let lines = source.split('\n').collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;
    let mut fence: Option<(char, usize)> = None;
    while index < lines.len() {
        let line = lines[index].trim_end_matches('\r');
        let trimmed = line.trim_start();
        if let Some((marker, length)) = fence {
            let run = trimmed.chars().take_while(|ch| *ch == marker).count();
            if run >= length && trimmed[run..].trim().is_empty() {
                fence = None;
            } else {
                result.push(Line::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::LightGreen),
                ));
            }
            index += 1;
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = if trimmed.starts_with("```") { '`' } else { '~' };
            let length = trimmed.chars().take_while(|ch| *ch == marker).count();
            let lang = trimmed[length..].trim();
            result.push(Line::styled(
                if lang.is_empty() {
                    "  Code".to_owned()
                } else {
                    format!("  Code · {lang}")
                },
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ));
            fence = Some((marker, length));
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
            if !result.is_empty() {
                result.push(Line::default());
            }
            let mut spans = vec![Span::styled("▍ ", style)];
            spans.extend(inline(content, style, 0));
            result.push(Line::from(spans));
        } else if let Some(content) = trimmed.strip_prefix("> ") {
            let style = Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC);
            let mut spans = vec![Span::styled("│ ", style)];
            spans.extend(inline(content, style, 0));
            result.push(Line::from(spans));
        } else if let Some(content) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let mut spans = vec![Span::styled(
                format!(
                    "{}• ",
                    " ".repeat(Line::raw(&line[..line.len() - trimmed.len()]).width())
                ),
                Style::default().fg(Color::Cyan),
            )];
            spans.extend(inline(content, Style::default(), 0));
            result.push(Line::from(spans));
        } else if let Some((number, content)) = ordered(trimmed) {
            let mut spans = vec![Span::styled(
                format!(
                    "{}{} ",
                    " ".repeat(Line::raw(&line[..line.len() - trimmed.len()]).width()),
                    number
                ),
                Style::default().fg(Color::Cyan),
            )];
            spans.extend(inline(content, Style::default(), 0));
            result.push(Line::from(spans));
        } else if matches!(trimmed, "---" | "***" | "___") {
            result.push(Line::styled(
                "─────────────────────────",
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            result.push(Line::from(inline(line, Style::default(), 0)));
        }
        index += 1;
    }
    if truncated {
        result.push(Line::raw(TRUNCATED));
    }
    Text::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .concat()
    }

    #[test]
    fn headings_emphasis_and_code_are_styled() {
        let text = render("# Plan\n- **Bold** and *italic* with `code`");
        assert_eq!(shown(&text.lines[0]), "▍ Plan");
        assert_eq!(shown(&text.lines[1]), "• Bold and italic with code");
        assert!(text.lines[1]
            .spans
            .iter()
            .any(|s| s.content == "Bold" && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(text.lines[1]
            .spans
            .iter()
            .any(|s| s.content == "italic" && s.style.add_modifier.contains(Modifier::ITALIC)));
        assert!(text.lines[1]
            .spans
            .iter()
            .any(|s| s.content == "code" && s.style.fg == Some(Color::Yellow)));
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

#[cfg(test)]
mod hardening_tests {
    use super::*;
    use ratatui::{
        backend::TestBackend,
        widgets::{Paragraph, Wrap},
        Terminal,
    };

    fn plain(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn safe(text: &Text<'_>) {
        for span in text.lines.iter().flat_map(|l| &l.spans) {
            assert!(
                span.content.chars().all(|c| !c.is_control()),
                "control in {:?}",
                span.content
            );
            assert!(!span.content.contains('\u{202e}'));
        }
    }
    #[test]
    fn controls_are_visible_in_markdown_and_literal_previews() {
        let payload =
            "\x1b[2J\x1b]8;;https://example.test\x07link\x1b]8;;\x1b\\\u{009b}31m\x08\r\u{202e}";
        let source=format!("# {payload}\n> {payload}\n`{payload}`\n```\n{payload}\n```\n| A | B |\n| --- | --- |\n| {payload} | end |");
        for text in [render(&source), raw(&source)] {
            safe(&text);
            let shown = plain(&text);
            for control in [
                "\\u{001b}",
                "\\u{0007}",
                "\\u{009b}",
                "\\u{0008}",
                "\\u{000d}",
                "\\u{202e}",
            ] {
                assert!(shown.contains(control), "missing {control}");
            }
        }
        assert!(
            source.contains('\x1b'),
            "original source must remain unchanged"
        );
    }
    #[test]
    fn crlf_tabs_and_international_text_have_explicit_display_semantics() {
        let source = "résumé 漢字 e\u{301} 👩‍💻 العربية עברית\r\n\tindent\rreturn";
        let expected = "résumé 漢字 e\u{301} 👩‍💻 العربية עברית\n    indent\\u{000d}return";
        assert_eq!(plain(&raw(source)), expected);
        assert_eq!(plain(&render(source)), expected);
    }
    #[test]
    fn tables_align_cjk_combining_marks_emoji_and_styled_cells() {
        let text=render("| Name | Result |\n| --- | --- |\n| 漢字 | **大** |\n| e\u{301} | `ok` |\n| 👩‍💻 | yes |");
        let widths = text
            .lines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| l.width())
            .collect::<Vec<_>>();
        assert!(
            widths.iter().all(|w| *w == widths[0]),
            "ragged widths: {widths:?}"
        );
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal
            .draw(|f| f.render_widget(Paragraph::new(text), f.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let borders = (0..40)
            .filter(|x| buffer[(*x, 0)].symbol() == "│")
            .collect::<Vec<_>>();
        for y in [2, 3, 4] {
            let row = (0..40)
                .filter(|x| buffer[(*x, y)].symbol() == "│")
                .collect::<Vec<_>>();
            assert_eq!(row, borders, "misaligned row {y}");
        }
    }
    #[test]
    fn longer_fences_do_not_close_on_shorter_or_nonempty_runs() {
        let text =
            render("````md\n```\n**still code**\n```` trailing\n# still code\n````\n# Heading");
        let shown = plain(&text);
        assert!(shown.contains("  ```"));
        assert!(shown.contains("  **still code**"));
        assert!(shown.contains("  ```` trailing"));
        assert!(shown.contains("  # still code"));
        assert!(shown.contains("▍ Heading"));
    }
    #[test]
    fn oversized_lines_documents_and_control_expansion_are_bounded() {
        let documents = [
            "é".repeat(MAX_LINE_BYTES),
            "short\n".repeat(MAX_LINES + 100),
            format!("{}\n", "word ".repeat(1000)).repeat(100),
            "\x1b".repeat(MAX_BYTES),
        ];
        for source in documents {
            for text in [render(&source), raw(&source)] {
                safe(&text);
                let shown = plain(&text);
                assert!(shown.ends_with(TRUNCATED));
                assert!(shown.len() < MAX_BYTES + TRUNCATED.len() + 2);
                assert!(text.lines.len() <= MAX_LINES + 1);
            }
        }
        let exact = "é".repeat(MAX_LINE_BYTES / 2);
        assert!(!plain(&raw(&exact)).contains(TRUNCATED));
    }
    #[test]
    fn sparse_table_padding_and_excess_columns_do_not_amplify_unboundedly() {
        let count = MAX_COLUMNS;
        let head = vec!["header"; count].join("|");
        let rule = vec!["---"; count].join("|");
        let wide = "W".repeat(120);
        let rows = (0..count)
            .map(|selected| {
                (0..count)
                    .map(|col| if col == selected { wide.as_str() } else { "x" })
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("{head}\n{rule}\n{rows}");
        assert!(plain(&render(&source)).len() < 32_000);
        let excessive = vec!["x"; MAX_COLUMNS + 1].join("|");
        let separators = vec!["---"; MAX_COLUMNS + 1].join("|");
        let literal = format!("{excessive}\n{separators}");
        assert_eq!(plain(&render(&literal)), literal);
    }
    #[test]
    fn adversarial_markup_and_unicode_boundaries_render_at_multiple_widths() {
        let source = format!(
            "{}\n{}\n{}",
            "* _ ** __ ` \\ [ 漢 e\u{301} 👩‍💻 ".repeat(120),
            "# heading\n".repeat(100),
            "```\nunclosed fence"
        );
        for width in [1, 20, 40, 80] {
            let text = render(&source);
            safe(&text);
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            terminal
                .draw(|f| {
                    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), f.area())
                })
                .unwrap();
        }
        let nested = format!("{}text{}", "**_".repeat(100), "_**".repeat(100));
        safe(&render(&nested));
    }
}
