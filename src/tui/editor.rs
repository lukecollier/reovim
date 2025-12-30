use std::{cell::RefCell, rc::Rc, str::FromStr};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent},
    style::Color,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    event::ReovimEvent,
    tui::{Component, Formatting, LayoutMode, Measurement, Overflow, status::StatusComponent},
};

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

struct TextRow {
    line_number: u16,
    vcs_status: VcsStatus,
    selected: bool,
    content: Rc<RefCell<String>>,
}

impl TextRow {
    pub fn from_str(line_number: u16, content: Rc<RefCell<String>>) -> anyhow::Result<Self> {
        Ok(Self {
            line_number,
            vcs_status: VcsStatus::None,
            selected: false,
            content: content,
        })
    }
}

impl Component for TextRow {
    fn render(&self, buffer: &mut super::terminal_buffer::TerminalBuffer) -> anyhow::Result<()> {
        // Just write the content - let the composite functions handle wrapping
        buffer.write(&*self.content.borrow());
        Ok(())
    }

    fn update(
        &mut self,
        _event: ReovimEvent,
        commands: &mut super::tree::ComponentCommands,
    ) -> Result<bool> {
        if self.selected == commands.has_focus() {
            Ok(false)
        } else if commands.has_focus() {
            self.selected = true;
            Ok(true)
        } else {
            self.selected = true;
            Ok(true)
        }
    }

    fn get_row_width(&self, row: u16, render_width: u16) -> u16 {
        let content = self.content.borrow();
        // Split content by render width and return the actual width of the specific row
        let rows = split_by_width(&content, render_width);
        if let Some(row_content) = rows.get(row as usize) {
            row_content.width() as u16
        } else {
            render_width
        }
    }

    fn cursor_bounds(
        &self,
        _width: u16,
        _height: u16,
        _formatting: &Formatting,
    ) -> (u16, u16, u16, u16) {
        // TextRow is a single logical line, treated as a single entity for navigation
        // Wrapping is visual only and doesn't affect navigation boundaries
        // max_col is the logical content width
        let max_col = (self.content.borrow().width() as u16).saturating_sub(1);
        (0, max_col, 0, 0)
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_width: Measurement::Fill,
            preferred_height: Measurement::Content,
            overflow_x: Overflow::Wrap,
            overflow_y: Overflow::Hide,
            request_focus: false,
            layout_mode: LayoutMode::VerticalSplit,
            focusable: true,
            ..Default::default()
        }
    }
}

pub struct EditableText {
    lines: Vec<Rc<RefCell<String>>>,
}

impl EditableText {
    fn from_str(content: &str) -> Result<Self> {
        let lines = content
            .lines()
            .map(|line: &str| Rc::new(RefCell::new(line.to_string())))
            .collect();
        Ok(Self { lines })
    }
}

impl Component for EditableText {
    fn children(&mut self, commands: &mut super::tree::ComponentCommands) -> anyhow::Result<()> {
        for line in &self.lines {
            commands.add_component(TextRow::from_str(0, line.clone())?)?;
        }
        Ok(())
    }
    fn update(
        &mut self,
        event: crate::event::ReovimEvent,
        commands: &mut super::tree::ComponentCommands,
    ) -> Result<bool> {
        if commands.has_focus() {
            match event {
                // vi motions
                ReovimEvent::Key(KeyEvent {
                    code,
                    modifiers: _,
                    kind: _,
                    state: _,
                }) => {
                    match code {
                        KeyCode::Char('t') => {
                            for line in self.lines.iter_mut() {
                                *line.borrow_mut() = String::from_str("lol")?;
                            }
                            return Ok(true); // Component changed, needs re-render
                        }
                        KeyCode::Char('l') => {
                            commands.move_cursor(1, 0);
                            return Ok(false); // Component changed, needs re-render
                        }
                        KeyCode::Char('h') => {
                            commands.move_cursor(-1, 0);
                            return Ok(false); // Component changed, needs re-render
                        }
                        KeyCode::Char('j') => {
                            commands.move_cursor(0, 1);
                            return Ok(false); // Component changed, needs re-render
                        }
                        KeyCode::Char('k') => {
                            commands.move_cursor(0, -1);
                            return Ok(false); // Component changed, needs re-render
                        }
                        _ => return Ok(false),
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn cursor_bounds(
        &self,
        _width: u16,
        _height: u16,
        _formatting: &Formatting,
    ) -> (u16, u16, u16, u16) {
        // EditableText has one child (TextRow) per line
        let line_count = self.lines.len();
        let max_row = if line_count > 0 {
            (line_count - 1) as u16
        } else {
            0
        };
        // max_col is wide open since children will be TextRows with their own widths
        (0, u16::MAX, 0, max_row)
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_width: Measurement::Fill,
            preferred_height: Measurement::Fill,
            overflow_x: Overflow::Wrap,
            overflow_y: Overflow::Scroll,
            request_focus: false,
            layout_mode: LayoutMode::VerticalSplit,
            ..Default::default()
        }
    }
}

pub struct Editor {
    content: String,
    file_name: String,
}

impl Editor {
    pub fn new(content: String, file_name: &str) -> Self {
        Self {
            content,
            file_name: file_name.to_string(),
        }
    }
}

impl Component for Editor {
    fn children(&mut self, commands: &mut super::tree::ComponentCommands) -> anyhow::Result<()> {
        commands.add_component(EditableText::from_str(&self.content)?)?;
        commands.add_component(StatusComponent::new(self.file_name.clone()))?;
        Ok(())
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_width: Measurement::Fill,
            preferred_height: Measurement::Fill,
            overflow_x: Overflow::Wrap,
            overflow_y: Overflow::Scroll,
            request_focus: true,
            layout_mode: LayoutMode::VerticalSplit,
            ..Default::default()
        }
    }

    fn update(
        &mut self,
        _event: crate::event::ReovimEvent,
        _commands: &mut super::tree::ComponentCommands,
    ) -> Result<bool> {
        return Ok(false);
    }

    fn render(&self, _buffer: &mut super::terminal_buffer::TerminalBuffer) -> anyhow::Result<()> {
        Ok(())
    }
}
