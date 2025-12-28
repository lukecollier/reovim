use crate::{
    event::ReovimEvent,
    tui::{Component, Formatting, Measurement, Overflow, terminal_buffer::TerminalBuffer},
};

use anyhow::Result;
use crossterm::style::Color;

pub struct StatusComponent<'a> {
    /// The file name for the bottom left.
    file_name: &'a str,
}

impl<'a> StatusComponent<'a> {
    pub fn new(file_name: &'a str) -> Self {
        StatusComponent {
            file_name: file_name,
        }
    }
}

fn pad_or_truncate(s: &str, width: u16) -> String {
    if s.len() >= width as usize {
        s[..width as usize].to_string()
    } else {
        format!("{:<width$}", s, width = width as usize)
    }
}

impl<'a> Component for StatusComponent<'a> {
    fn render(&self, buffer: &mut TerminalBuffer) -> Result<()> {
        let status_line_str = pad_or_truncate(self.file_name, buffer.width());
        buffer
            .set_background(Color::Black)
            .set_foreground(Color::Yellow)
            .write(&status_line_str);
        Ok(())
    }

    fn update(
        &mut self,
        _event: ReovimEvent,
        _commands: &mut super::tree::ComponentCommands,
    ) -> Result<bool> {
        Ok(false)
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: Measurement::Fill,
            preferred_height: Measurement::Cell(1),
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: false,
        }
    }
}
