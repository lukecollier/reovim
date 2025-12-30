use std::str::FromStr;

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent},
    style::Color,
};

use crate::{
    event::ReovimEvent,
    tui::{
        Component, Formatting, LayoutMode, Measurement, Overflow,
        status::StatusComponent,
    },
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

struct TextRow {
    line_number: u16,
    vcs_status: VcsStatus,
    selected: bool,
    content: String,
}

impl TextRow {
    pub fn from_str(line_number: u16, content: &str) -> anyhow::Result<Self> {
        Ok(Self {
            line_number,
            vcs_status: VcsStatus::None,
            selected: false,
            content: String::from_str(content)?,
        })
    }
}

impl Component for TextRow {
    fn render(&self, buffer: &mut super::terminal_buffer::TerminalBuffer) -> anyhow::Result<()> {
        // Just write the content - let the composite functions handle wrapping
        buffer.write(&self.content);
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

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_width: Measurement::Content,
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
    content: String,
}

impl EditableText {
    fn from_str(content: &str) -> Result<Self> {
        Ok(Self {
            content: String::from_str(content)?,
        })
    }
}

impl Component for EditableText {
    fn children(&mut self, commands: &mut super::tree::ComponentCommands) -> anyhow::Result<()> {
        for line in self.content.lines() {
            commands.add_component(TextRow::from_str(0, line)?)?;
        }
        Ok(())
    }
    fn update(
        &mut self,
        _event: crate::event::ReovimEvent,
        _commands: &mut super::tree::ComponentCommands,
    ) -> Result<bool> {
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
                    return Ok(true); // Component changed, needs re-render
                }
                _ => {}
            }
        }
        return Ok(false);
    }

    fn render(&self, _buffer: &mut super::terminal_buffer::TerminalBuffer) -> anyhow::Result<()> {
        Ok(())
    }
}
