use crate::{
    event::ReovimEvent,
    tui::{Component, Formatting, Measurement, Overflow, terminal_buffer::TerminalBuffer},
};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    style::Color,
};

pub struct GutterComponent {
    width: usize,
    start_from: u16,
}

impl GutterComponent {
    pub fn new(width: usize, start_from: u16) -> Self {
        GutterComponent { width, start_from }
    }
}

fn pad_or_truncate(s: &str, width: u16) -> String {
    if s.len() >= width as usize {
        s[..width as usize].to_string()
    } else {
        format!("{:>width$}", s, width = width as usize)
    }
}

impl<'a> Component for GutterComponent {
    fn render(&self, buffer: &mut TerminalBuffer) -> Result<()> {
        for i in 0..buffer.height() {
            let number = i + self.start_from;
            let row_number = pad_or_truncate(&(number.to_string()), self.width as u16);
            // renders a symbol on the line number, will use Lsp events for this
            buffer
                .set_background(Color::Black)
                .set_foreground(Color::Yellow);
            buffer.write(&"│ ");
            buffer
                .set_background(Color::Black)
                .set_foreground(Color::Green);
            buffer.write(&row_number);
            buffer
                .set_background(Color::Black)
                .set_foreground(Color::Green);
            buffer.write(&" ");
            buffer.newline();
        }
        Ok(())
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: Measurement::Cell(self.width + 3),
            preferred_height: Measurement::Percent(100),
            overflow_x: Overflow::Wrap,
            overflow_y: Overflow::Hide,
            request_focus: false,
        }
    }
}
