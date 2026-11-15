//! Shared grapheme-aware input editing and viewport rendering.
use super::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub(super) struct Editor {
    cursor: Option<usize>, // None follows the end, including newly restored drafts.
    width: Cell<usize>,
    preferred_column: Option<usize>,
}
#[derive(Clone)]
struct Position {
    byte: usize,
    row: usize,
    column: usize,
}
fn layout(text: &str, width: usize) -> (Vec<String>, Vec<Position>) {
    let width = width.max(2);
    let mut lines = vec![String::new()];
    let mut positions = Vec::new();
    let mut column = 0;
    let mut just_wrapped = false;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let display = if grapheme == "\t" { "    " } else { grapheme };
        let size = UnicodeWidthStr::width(display).min(width);
        if display != "\n" && column + size > width {
            lines.push(String::new());
            column = 0;
        }
        positions.push(Position {
            byte,
            row: lines.len() - 1,
            column,
        });
        if display == "\n" {
            if !just_wrapped {
                lines.push(String::new());
            }
            column = 0;
            just_wrapped = false;
        } else {
            just_wrapped = false;
            lines
                .last_mut()
                .unwrap()
                .push_str(&safe_display(display).replace('\t', "    "));
            column += size;
            if column >= width {
                lines.push(String::new());
                column = 0;
                just_wrapped = true;
            }
        }
    }
    positions.push(Position {
        byte: text.len(),
        row: lines.len() - 1,
        column,
    });
    (lines, positions)
}
impl Editor {
    pub(super) fn reset(&mut self) {
        self.cursor = None;
        self.preferred_column = None;
    }
    fn position(&self, text: &str) -> usize {
        let byte = self.cursor.unwrap_or(text.len()).min(text.len());
        text.grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .take_while(|i| *i <= byte)
            .last()
            .unwrap_or(0)
    }
    pub(super) fn key(&mut self, text: &mut String, key: KeyEvent, multiline: bool, limit: usize) {
        if !matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.preferred_column = None;
        }
        let at = self.position(text);
        let (_, positions) = layout(
            text,
            if self.width.get() == 0 {
                80
            } else {
                self.width.get()
            },
        );
        let index = positions.iter().position(|p| p.byte == at).unwrap();
        let previous = positions[index.saturating_sub(1)].byte;
        let next = positions[(index + 1).min(positions.len() - 1)].byte;
        let insert = match key.code {
            KeyCode::Enter
                if multiline
                    && key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                Some('\n')
            }
            KeyCode::Char('j') if multiline && key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some('\n')
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                Some(c)
            }
            _ => None,
        };
        if let Some(c) = insert {
            if text.len() + c.len_utf8() <= limit {
                text.insert(at, c);
                self.cursor = Some(at + c.len_utf8());
            }
            return;
        }
        match key.code {
            KeyCode::Left => self.cursor = Some(previous),
            KeyCode::Right => self.cursor = Some(next),
            KeyCode::Backspace if at > 0 => {
                text.replace_range(previous..at, "");
                self.cursor = Some(previous);
            }
            KeyCode::Delete if at < text.len() => {
                text.replace_range(at..next, "");
                self.cursor = Some(at);
            }
            KeyCode::Home | KeyCode::End => {
                let current = &positions[index];
                let target = if key.modifiers.contains(KeyModifiers::CONTROL) {
                    if key.code == KeyCode::Home {
                        0
                    } else {
                        text.len()
                    }
                } else {
                    let mut row = positions.iter().filter(|p| p.row == current.row);
                    if key.code == KeyCode::Home {
                        row.clone().next().unwrap().byte
                    } else {
                        row.next_back().unwrap().byte
                    }
                };
                self.cursor = Some(target);
            }
            KeyCode::Up | KeyCode::Down if multiline => {
                let current = &positions[index];
                let column = *self.preferred_column.get_or_insert(current.column);
                let row = if key.code == KeyCode::Up {
                    current.row.saturating_sub(1)
                } else {
                    current.row + 1
                };
                if let Some(target) = positions
                    .iter()
                    .filter(|p| p.row == row)
                    .min_by_key(|p| p.column.abs_diff(column))
                {
                    self.cursor = Some(target.byte);
                }
            }
            _ => {}
        }
    }
    pub(super) fn draw(
        &self,
        frame: &mut Frame,
        text: &str,
        placeholder: &str,
        area: ratatui::layout::Rect,
        focused: bool,
    ) {
        if area.width < 2 || area.height == 0 {
            return;
        }
        self.width.set(area.width as usize);
        let (lines, positions) = layout(text, area.width as usize);
        let caret = self.position(text);
        let position = positions.iter().find(|p| p.byte == caret).unwrap();
        let offset = position.row.saturating_sub(area.height as usize - 1);
        let content = if text.is_empty() {
            placeholder.to_owned()
        } else {
            lines
                .into_iter()
                .skip(offset)
                .take(area.height as usize)
                .collect::<Vec<_>>()
                .join("\n")
        };
        frame.render_widget(
            Paragraph::new(content).style(Style::default().fg(if text.is_empty() {
                Color::DarkGray
            } else {
                Color::Green
            })),
            area,
        );
        if focused {
            frame.set_cursor_position((
                area.x + position.column as u16,
                area.y + (position.row - offset) as u16,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(editor: &mut Editor, text: &mut String, code: KeyCode) {
        editor.key(text, KeyEvent::new(code, KeyModifiers::NONE), true, 8192);
    }
    #[test]
    fn movement_and_deletion_preserve_graphemes_and_support_wrapped_rows() {
        let mut editor = Editor::default();
        editor.width.set(8);
        let mut text = "café👩‍💻\nsecond".to_owned();
        key(&mut editor, &mut text, KeyCode::Home);
        key(&mut editor, &mut text, KeyCode::Up);
        key(&mut editor, &mut text, KeyCode::Right);
        key(&mut editor, &mut text, KeyCode::Delete);
        assert_eq!(text, "cfé👩‍💻\nsecond");
        key(&mut editor, &mut text, KeyCode::Char('a'));
        assert_eq!(text, "café👩‍💻\nsecond");
        key(&mut editor, &mut text, KeyCode::End);
        key(&mut editor, &mut text, KeyCode::Backspace);
        assert!(!text.contains('👩'));
        assert!(text.contains("café"));
        editor.width.set(6);
        let mut text = "123456abcdef".to_owned();
        editor.reset();
        key(&mut editor, &mut text, KeyCode::Up);
        key(&mut editor, &mut text, KeyCode::Char('X'));
        assert_eq!(text, "123456Xabcdef");
    }
    #[test]
    fn unicode_limit_and_modified_enter_do_not_submit() {
        let mut editor = Editor::default();
        let mut text = "e\u{301}".to_owned();
        key(&mut editor, &mut text, KeyCode::Left);
        key(&mut editor, &mut text, KeyCode::Delete);
        assert!(text.is_empty());
        editor.key(
            &mut text,
            KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE),
            true,
            1,
        );
        assert!(text.is_empty());
        editor.key(
            &mut text,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            true,
            8,
        );
        assert_eq!(text, "\n");
    }
    #[test]
    fn caret_tracks_scrolled_text_and_input_has_its_own_color() {
        use ratatui::{backend::TestBackend, Terminal};
        let editor = Editor::default();
        let text = "first\nsecond\nlast";
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).unwrap();
        terminal
            .draw(|f| editor.draw(f, text, "", f.area(), true))
            .unwrap();
        use ratatui::backend::Backend;
        assert_eq!(
            terminal.backend_mut().get_cursor_position().unwrap(),
            ratatui::layout::Position::new(4, 1)
        );
        assert_eq!(terminal.backend().buffer()[(0, 1)].fg, Color::Green);
    }
    #[test]
    fn vertical_navigation_keeps_column_through_short_lines_and_exact_wraps() {
        let mut editor = Editor::default();
        editor.width.set(20);
        let mut text = "abcdefgh\nx\n12345678".to_owned();
        key(&mut editor, &mut text, KeyCode::Up);
        key(&mut editor, &mut text, KeyCode::Up);
        key(&mut editor, &mut text, KeyCode::Char('!'));
        assert_eq!(text, "abcdefgh!\nx\n12345678");
        let (lines, positions) = layout("123456\nnext", 6);
        assert_eq!(lines, vec!["123456", "next"]);
        assert_eq!(positions.last().unwrap().row, 1);
    }
}
