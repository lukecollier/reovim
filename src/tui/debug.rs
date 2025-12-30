use crate::tui::{Component, Formatting, LayoutMode, Measurement, Overflow, terminal_buffer::TerminalBuffer};

use anyhow::Result;
use crossterm::style::Color;

pub struct DebugComponent {
    color: Color,
    width: Measurement,
    height: Measurement,
}

impl DebugComponent {
    pub fn with_width(color: Color, width: Measurement) -> Self {
        DebugComponent {
            color,
            width,
            height: Measurement::Fill,
        }
    }
    pub fn with_height(color: Color, height: Measurement) -> Self {
        DebugComponent {
            color,
            width: Measurement::Fill,
            height,
        }
    }
    pub fn new(color: Color) -> Self {
        DebugComponent {
            color,
            width: Measurement::Fill,
            height: Measurement::Fill,
        }
    }
}

impl<'a> Component for DebugComponent {
    fn render(&self, buffer: &mut TerminalBuffer) -> Result<()> {
        buffer.set_background(self.color);

        // Fill the entire buffer with the color
        for _ in 0..buffer.height() {
            for _ in 0..buffer.width() {
                buffer.write(" ");
            }
            buffer.newline();
        }

        buffer.set_background(Color::Reset);
        Ok(())
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: self.width,
            preferred_height: self.height,
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: false,
            layout_mode: LayoutMode::VerticalSplit,
            focusable: true,
        }
    }
}
