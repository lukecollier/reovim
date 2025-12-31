use crate::tui::{
    Component, Formatting, LayoutMode, Measurement, Overflow, terminal_buffer::TerminalBuffer,
};

use anyhow::Result;
use crossterm::style::Color;

pub struct StatusComponent {
    /// The file name for the bottom left.
    file_name: String,
}

impl StatusComponent {
    pub fn new(file_name: String) -> Self {
        StatusComponent { file_name }
    }
}

fn pad_or_truncate(s: &str, width: u16) -> String {
    if s.len() >= width as usize {
        s[..width as usize].to_string()
    } else {
        format!("{:<width$}", s, width = width as usize)
    }
}

impl Component for StatusComponent {
    fn render(&self, buffer: &mut TerminalBuffer, _query: crate::tui::ComponentQuery) -> Result<()> {
        let status_line_str = pad_or_truncate(&self.file_name, buffer.width());
        buffer
            .set_background(Color::Black)
            .set_foreground(Color::Yellow)
            .write(&status_line_str);
        Ok(())
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
            layout_mode: LayoutMode::VerticalSplit,
            focusable: false,
        }
    }
}
