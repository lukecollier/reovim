use std::cell::Cell;

use crate::{
    event::ReovimEvent,
    tui::{Component, Formatting, Measurement, Overflow, terminal_buffer::TerminalBuffer},
};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind},
    style::Color,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

enum VcsStatus {
    Add,
    None,
    Deleted,
    Modified,
}

impl VcsStatus {
    fn color(&self) -> Color {
        match self {
            VcsStatus::Add => Color::Green,
            VcsStatus::None => Color::Reset,
            VcsStatus::Deleted => Color::Red,
            VcsStatus::Modified => Color::DarkYellow,
        }
    }
}

enum Line<'a> {
    Multi {
        line_number: u16,
        vcs_status: VcsStatus,
        content: Vec<&'a str>,
        selected: bool,
    },
    Single {
        line_number: u16,
        vcs_status: VcsStatus,
        content: &'a str,
        selected: bool,
    },
}

impl<'a> Line<'a> {
    fn new(line_number: u16, content: &'a str, row_size: u16, gutter_width: u16) -> Line<'a> {
        if (content.width() as u16 + gutter_width) > row_size {
            let row = split_by_width(content, row_size - gutter_width);
            Line::Multi {
                line_number,
                vcs_status: VcsStatus::None,
                content: row,
                selected: false,
            }
        } else {
            Line::Single {
                line_number,
                vcs_status: VcsStatus::None,
                content,
                selected: false,
            }
        }
    }

    fn height(&self) -> u16 {
        match self {
            Line::Multi {
                line_number: _,
                vcs_status: _,
                content,
                selected: _,
            } => content.len() as u16,
            Line::Single {
                line_number: _,
                vcs_status: _,
                content: _,
                selected: _,
            } => 1,
        }
    }

    fn render(
        &self,
        buffer: &mut TerminalBuffer,
        gutter_size: u16,
        skip_lines: u16,
        max_rows: u16,
    ) -> u16 {
        match self {
            Line::Multi {
                line_number,
                vcs_status,
                content,
                selected,
            } => {
                let foreground_color = if *selected {
                    Color::DarkYellow
                } else {
                    Color::DarkGrey
                };
                let mut rendered = 0u16;
                for (idx, line) in content.iter().enumerate() {
                    // Skip lines before the offset
                    if (idx as u16) < skip_lines {
                        continue;
                    }
                    // Stop if we've filled the buffer
                    if rendered >= max_rows {
                        break;
                    }
                    // Only show gutter on first line (idx == 0) or continuation that's being rendered
                    if idx == 0 {
                        // First line of multi-line
                        buffer
                            .set_background(Color::Reset)
                            .set_foreground(vcs_status.color())
                            .write(&"│")
                            .set_background(Color::Reset)
                            .set_foreground(foreground_color)
                            .write(&pad_or_truncate(
                                &line_number.to_string(),
                                gutter_size.saturating_sub(2),
                            ))
                            .set_background(Color::Reset)
                            .set_foreground(Color::Reset)
                            .write(&" ")
                            .write(line)
                            .newline();
                    } else {
                        // Continuation lines
                        buffer
                            .set_background(Color::Reset)
                            .set_foreground(Color::Reset)
                            .write(&" ".repeat(gutter_size.into()))
                            .write(line)
                            .newline();
                    }
                    rendered += 1;
                }
                rendered
            }
            Line::Single {
                line_number,
                vcs_status,
                content,
                selected,
            } => {
                if skip_lines == 0 && max_rows > 0 {
                    let foreground_color = if *selected {
                        Color::DarkYellow
                    } else {
                        Color::DarkGrey
                    };
                    buffer
                        .set_background(Color::Reset)
                        .set_foreground(vcs_status.color())
                        .write(&"│")
                        .set_background(Color::Reset)
                        .set_foreground(foreground_color)
                        .write(&pad_or_truncate(
                            &line_number.to_string(),
                            gutter_size.saturating_sub(2),
                        ))
                        .set_background(Color::Reset)
                        .set_foreground(Color::Reset)
                        .write(&" ")
                        .write(content)
                        .newline();
                    1
                } else {
                    0
                }
            }
        }
    }
}

pub struct TextComponent<'a> {
    content: &'a str,
    lines: Vec<Line<'a>>,
    line_size: u16,
    show_gutter: bool,
    last_cursor_row: Cell<u16>,
}

impl<'a> TextComponent<'a> {
    pub fn new(content: &'a str, max_width: u16) -> Self {
        let lines: Vec<_> = content
            .lines()
            .enumerate()
            .map(|(idx, str)| Line::new(idx as u16 + 1, str, max_width, 5))
            .collect();
        // we need to figure out the lines here
        TextComponent {
            content,
            line_size: lines.len() as u16,
            lines,
            show_gutter: true,
            last_cursor_row: Cell::new(0),
        }
    }

    fn update_lines(&mut self, max_width: u16) {
        let lines: Vec<_> = self
            .content
            .lines()
            .enumerate()
            .map(|(idx, str)| Line::new(idx as u16 + 1, str, max_width, 5))
            .collect();
        self.lines = lines;
    }
}

fn pad_or_truncate(s: &str, width: u16) -> String {
    if s.len() >= width as usize {
        s[..width as usize].to_string()
    } else {
        format!("{:>width$}", s, width = width as usize)
    }
}

/// Split text into chunks of max width, respecting unicode character widths
fn split_by_width(text: &str, max_width: u16) -> Vec<&str> {
    let max_width = max_width as usize;
    let mut chunks = Vec::new();
    let mut current_width = 0;
    let mut start_byte = 0;

    for (byte_pos, ch) in text.char_indices() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width + ch_width > max_width && start_byte < byte_pos {
            // Exceeded width, slice from start_byte to byte_pos
            chunks.push(&text[start_byte..byte_pos]);
            start_byte = byte_pos;
            current_width = ch_width;
        } else {
            current_width += ch_width;
        }
    }

    // Add remaining text
    if start_byte < text.len() {
        chunks.push(&text[start_byte..]);
    }

    chunks
}

impl<'a> TextComponent<'a> {
    /// Calculate gutter width (line number + divider + space)
    fn gutter_width(&self) -> u16 {
        if !self.show_gutter {
            return 0;
        }
        let line_number_width = self.lines.len().to_string().width() as u16;
        line_number_width + 2 // +1 for divider, +1 for space
    }
}

impl<'a> Component for TextComponent<'a> {
    fn render(&self, buffer: &mut TerminalBuffer) -> Result<()> {
        // Get scroll offset from buffer
        let (_, scroll_y) = buffer.scroll();
        let start_at = scroll_y;

        // Get cursor position from buffer (set by tree)
        let (_, cursor_row) = buffer.cursor_position();
        self.last_cursor_row.set(cursor_row);

        let mut pos = 0u16;
        let mut buffer_rows_used = 0u16;

        for line in &self.lines {
            let line_height = line.height();

            // Skip lines that end before the scroll position
            if pos + line_height <= start_at as u16 {
                pos += line_height;
                continue;
            }

            // Calculate how many rows of this line to skip (due to scrolling)
            let skip_in_line = if pos < start_at as u16 {
                start_at as u16 - pos
            } else {
                0
            };

            // Calculate remaining buffer space
            let remaining_buffer = buffer.height().saturating_sub(buffer_rows_used);
            if remaining_buffer == 0 {
                break;
            }

            // Render this line (potentially partial)
            let rows_rendered =
                line.render(buffer, self.gutter_width(), skip_in_line, remaining_buffer);

            buffer_rows_used += rows_rendered;
            pos += line_height;

            if buffer_rows_used >= buffer.height() {
                break;
            }
        }
        Ok(())
    }

    fn update(
        &mut self,
        event: ReovimEvent,
        commands: &mut crate::tui::tree::ComponentCommands,
    ) -> Result<bool> {
        if commands.has_focus() {
            match event {
                ReovimEvent::Resize(row, _) => {
                    self.update_lines(row);
                    return Ok(true); // Component changed, needs re-render
                }
                ReovimEvent::Key(KeyEvent {
                    code: KeyCode::Char('l'),
                    modifiers: _,
                    kind: _,
                    state: _,
                }) => {
                    commands.move_cursor(1, 0);
                    return Ok(true); // Component changed, needs re-render
                }
                ReovimEvent::Key(KeyEvent {
                    code: KeyCode::Char('h'),
                    modifiers: _,
                    kind: _,
                    state: _,
                }) => {
                    commands.move_cursor(-1, 0);
                    return Ok(true); // Component changed, needs re-render
                }
                ReovimEvent::Key(KeyEvent {
                    code: KeyCode::Char('j'),
                    modifiers: _,
                    kind: _,
                    state: _,
                }) => {
                    let (col, row) = commands.get_cursor();
                    let new_row = (row + 1).min(self.lines.len() as u16 - 1);
                    commands.set_cursor(col, new_row);
                    return Ok(true); // Component changed, needs re-render
                }
                ReovimEvent::Key(KeyEvent {
                    code: KeyCode::Char('k'),
                    modifiers: _,
                    kind: _,
                    state: _,
                }) => {
                    commands.move_cursor(0, -1);
                    return Ok(true); // Component changed, needs re-render
                }
                _ => {}
            }
        }
        Ok(false) // No changes
    }

    fn scroll_bounds(&self) -> (usize, usize) {
        // Min scroll is 0, max scroll is the total lines minus 1
        let max_scroll = self
            .lines
            .iter()
            .fold(0u16, |acc, line| acc + line.height())
            .saturating_sub(1);
        (0, max_scroll as usize)
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: Measurement::Fill, // Leave room for gutter
            preferred_height: Measurement::Fill,
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Scroll,
            request_focus: true,
        }
    }
}
