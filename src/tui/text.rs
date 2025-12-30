use std::cell::Cell;

use crate::{
    event::ReovimEvent,
    tui::{
        Component, CursorStyle, Formatting, LayoutMode, Measurement, Overflow,
        terminal_buffer::TerminalBuffer,
    },
};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    style::Color,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

enum TextMode {
    Insert,
    Normal,
}

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

pub enum Content {
    Multi(Vec<String>),
    Single(String),
}

struct Line {
    line_number: u16,
    vcs_status: VcsStatus,
    selected: bool,
    content: Content,
}

impl Line {
    fn new(line_number: u16, content: &str, row_size: u16, gutter_width: u16) -> Line {
        if (content.width() as u16 + gutter_width) > row_size {
            let row = split_by_width(content, row_size - gutter_width)
                .iter()
                .map(|str| str.to_string())
                .collect();
            let multi = Content::Multi(row);
            Self {
                line_number,
                vcs_status: VcsStatus::None,
                content: multi,
                selected: false,
            }
        } else {
            Self {
                line_number,
                vcs_status: VcsStatus::None,
                content: Content::Single(content.to_string()),
                selected: false,
            }
        }
    }

    fn content_string(&self) -> String {
        match &self.content {
            Content::Multi(items) => items.join(""),
            Content::Single(item) => item.to_string(),
        }
    }

    fn height(&self) -> u16 {
        match &self.content {
            Content::Multi(content) => content.len() as u16,
            Content::Single(_) => 1,
        }
    }

    fn render(
        &self,
        buffer: &mut TerminalBuffer,
        gutter_size: u16,
        skip_lines: u16,
        max_rows: u16,
    ) -> u16 {
        match &self.content {
            Content::Multi(content) => {
                let foreground_color = if self.selected {
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
                            .set_foreground(self.vcs_status.color())
                            .write(&"│")
                            .set_background(Color::Reset)
                            .set_foreground(foreground_color)
                            .write(&pad_or_truncate(
                                &self.line_number.to_string(),
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
            Content::Single(content) => {
                if skip_lines == 0 && max_rows > 0 {
                    let foreground_color = if self.selected {
                        Color::DarkYellow
                    } else {
                        Color::DarkGrey
                    };
                    buffer
                        .set_background(Color::Reset)
                        .set_foreground(self.vcs_status.color())
                        .write(&"│")
                        .set_background(Color::Reset)
                        .set_foreground(foreground_color)
                        .write(&pad_or_truncate(
                            &self.line_number.to_string(),
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
    mode: TextMode,
    content: &'a str,
    lines: Vec<Line>,
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
            mode: TextMode::Normal,
            content,
            lines,
            show_gutter: true,
            last_cursor_row: Cell::new(0),
        }
    }

    fn get_end_of_line_offset(&self) -> u16 {
        match self.mode {
            TextMode::Insert => 0,
            TextMode::Normal => 1,
        }
    }

    fn is_normal(&self) -> bool {
        matches!(self.mode, TextMode::Normal)
    }

    fn is_insert(&self) -> bool {
        matches!(self.mode, TextMode::Insert)
    }

    fn update_lines(&mut self, max_width: u16) {
        let lines: Vec<_> = self
            .lines
            .iter()
            .map(|line| line.content_string())
            .enumerate()
            .map(|(idx, str)| Line::new(idx as u16 + 1, &str, max_width, 5))
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

    pub fn get_line_mut(&mut self, row: u16) -> Option<&mut String> {
        let mut pos = 0u16;

        for line in self.lines.iter_mut() {
            let line_height = line.height();

            // Check if row falls within this line
            if row >= pos && row < pos + line_height {
                match line.content {
                    Content::Single(ref mut content) => {
                        return Some(content);
                    }
                    Content::Multi(ref mut content) => {
                        // Calculate offset within the multi-line content
                        let offset = (row - pos) as usize;
                        return content.get_mut(offset);
                    }
                }
            }

            pos += line_height;
        }

        None
    }

    /// Get the content string at the given row number
    /// For single-line entries, returns the entire line content
    /// For multi-line (wrapped) entries, returns the specific wrapped segment
    /// Returns None if the row is out of bounds
    pub fn get_line(&'a self, row: u16) -> Option<&'a str> {
        let mut pos = 0u16;

        for line in &self.lines {
            let line_height = line.height();

            // Check if row falls within this line
            if row >= pos && row < pos + line_height {
                match &line.content {
                    Content::Single(content) => {
                        return Some(content.as_ref());
                    }
                    Content::Multi(content) => {
                        // Calculate offset within the multi-line content
                        let offset = (row - pos) as usize;
                        return content.get(offset).map(|string| string.as_ref());
                    }
                }
            }

            pos += line_height;
        }

        None
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
                ReovimEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: _,
                }) => {
                    let (mut local_col, local_row) = commands.global_to_local(column, row);
                    if let Some(contents) = self.get_line(local_row) {
                        local_col = local_col.min(
                            (contents.width() as u16 + self.gutter_width())
                                .saturating_sub(self.get_end_of_line_offset()),
                        );
                    }
                    commands.set_cursor(local_col, local_row);
                }

                ReovimEvent::Resize(row, _) => {
                    self.update_lines(row);
                    return Ok(true); // Component changed, needs re-render
                }
                ReovimEvent::Key(KeyEvent {
                    code,
                    modifiers: _,
                    kind: _,
                    state: _,
                }) if self.is_insert() => match code {
                    KeyCode::Esc => {
                        commands.move_cursor(-1, 0);
                        commands.set_cursor_style(CursorStyle::Block);
                        self.mode = TextMode::Normal
                    }
                    KeyCode::Backspace => {
                        let cursor = commands.get_cursor();
                        let gutter_width = self.gutter_width();
                        if cursor.col != gutter_width {
                            if let Some(contents) =
                                self.get_line_mut(cursor.row + commands.get_scroll_y() as u16)
                            {
                                contents.remove((cursor.col - gutter_width) as usize);
                            }
                            commands.move_cursor(-1, 0);
                            return Ok(true);
                        }
                    }
                    KeyCode::Char(character) => {
                        let cursor = commands.get_cursor();
                        let gutter_width = self.gutter_width();
                        if let Some(contents) =
                            self.get_line_mut(cursor.row + commands.get_scroll_y() as u16)
                        {
                            contents.insert((cursor.col - gutter_width) as usize, character);
                        }
                        commands.move_cursor(1, 0);
                        return Ok(true);
                    }
                    _ => {}
                },
                ReovimEvent::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: _,
                    kind: _,
                    state: _,
                }) if self.is_normal() => {
                    commands.move_cursor(1, 0);
                    commands.set_cursor_style(CursorStyle::Line);
                    self.mode = TextMode::Insert
                }
                ReovimEvent::Key(KeyEvent {
                    code: KeyCode::Char('i'),
                    modifiers: _,
                    kind: _,
                    state: _,
                }) if self.is_normal() => {
                    commands.set_cursor_style(CursorStyle::Line);
                    self.mode = TextMode::Insert
                }
                // vi motions
                ReovimEvent::Key(KeyEvent {
                    code,
                    modifiers: _,
                    kind: _,
                    state: _,
                }) if self.is_normal() => {
                    match code {
                        KeyCode::Char('l') => {
                            commands.move_cursor(1, 0);
                        }
                        KeyCode::Char('h') => {
                            commands.move_cursor(-1, 0);
                        }
                        KeyCode::Char('j') => {
                            commands.move_cursor(0, 1);
                        }
                        KeyCode::Char('k') => {
                            commands.move_cursor(0, -1);
                        }
                        _ => return Ok(false),
                    }
                    let cursor = commands.get_cursor();
                    if let Some(contents) =
                        self.get_line(cursor.row + commands.get_scroll_y() as u16)
                    {
                        commands.clamp_cursor_col(
                            self.gutter_width(),
                            (contents.width() as u16 + self.gutter_width())
                                .saturating_sub(self.get_end_of_line_offset()),
                        );
                    }

                    return Ok(true); // Component changed, needs re-render
                }
                _ => {}
            }
        }
        Ok(false) // No changes
    }

    fn cursor_bounds(
        &self,
        width: u16,
        height: u16,
        formatting: &crate::tui::Formatting,
    ) -> (u16, u16, u16, u16) {
        // (min_col, max_col, min_row, max_row)
        let gutter = self.gutter_width();

        // For vertical bounds: if Overflow::Scroll, allow cursor to move into content beyond visible area
        let max_row = if matches!(formatting.overflow_y, crate::tui::Overflow::Scroll) {
            // Allow scrolling: cursor can move to end of all content
            let total_height = self
                .lines
                .iter()
                .fold(0u16, |acc, line| acc + line.height());
            total_height.saturating_sub(1)
        } else {
            // Constrain to visible area
            height.saturating_sub(1)
        };

        // For horizontal bounds: similar logic
        let max_col = if matches!(formatting.overflow_x, crate::tui::Overflow::Scroll) {
            // Allow scrolling: unrestricted width
            u16::MAX
        } else {
            width.saturating_sub(1)
        };

        (gutter, max_col, 0, max_row)
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
            layout_mode: LayoutMode::VerticalSplit,
            focusable: true,
        }
    }
}
