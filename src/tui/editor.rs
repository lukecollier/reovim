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

struct TextGutter {
    line_number: u16,
    vcs_status: VcsStatus,
    width: usize,
}

impl TextGutter {
    fn new(line_number: u16, width: usize) -> Self {
        Self {
            line_number,
            vcs_status: VcsStatus::None,
            width,
        }
    }
}

impl Component for TextGutter {
    fn render(&self, buffer: &mut super::terminal_buffer::TerminalBuffer) -> anyhow::Result<()> {
        // Just write the content - let the composite functions handle wrapping
        let width = self.width;
        buffer
            .set_background(self.vcs_status.color())
            .write(&"│")
            .set_background(Color::Reset)
            .write(&format!("{:>width$} ", self.line_number));
        Ok(())
    }
    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_width: Measurement::Content,
            preferred_height: Measurement::Fill,
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: false,
            focusable: false,
            ..Default::default()
        }
    }
}

struct TextContent {
    selected: bool,
    content: Rc<RefCell<String>>,
}

impl TextContent {
    fn new(selected: bool, content: Rc<RefCell<String>>) -> Self {
        Self { selected, content }
    }
}

impl Component for TextContent {
    fn render(&self, buffer: &mut super::terminal_buffer::TerminalBuffer) -> anyhow::Result<()> {
        // Just write the content - let the composite functions handle wrapping
        if self.selected {
            buffer.set_background(Color::DarkGrey);
        } else {
            buffer.set_background(Color::Reset);
        }
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
            self.selected = false;
            Ok(true)
        }
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

struct TextRow {
    line_number: u16,
    vcs_status: VcsStatus,
    content: Rc<RefCell<String>>,
    line_number_width: usize,
}

impl TextRow {
    pub fn from_str(
        line_number: u16,
        content: Rc<RefCell<String>>,
        line_number_width: usize,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            line_number,
            vcs_status: VcsStatus::None,
            content: content,
            line_number_width,
        })
    }
}

impl Component for TextRow {
    fn children(&mut self, commands: &mut super::tree::ComponentCommands) -> Result<()> {
        commands.add_component(TextGutter::new(self.line_number, self.line_number_width))?;
        commands.add_component(TextContent::new(false, self.content.clone()))?;
        Ok(())
    }
    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_width: Measurement::Fill,
            preferred_height: Measurement::Content,
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: false,
            layout_mode: LayoutMode::HorizontalSplit,
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
        let line_number_width = self
            .lines
            .iter()
            .enumerate()
            .last()
            .map(|(idx, _line)| (idx + 1).to_string().len())
            .unwrap_or(0);
        for (idx, line) in self.lines.iter().enumerate() {
            let line_number = idx + 1;
            commands.add_component(TextRow::from_str(
                line_number as u16,
                line.clone(),
                line_number_width,
            )?)?;
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
