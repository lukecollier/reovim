use crate::tui::{Component, Formatting, Measurement, Overflow, terminal_buffer::TerminalBuffer};

use anyhow::Result;

pub struct CommandComponent {
    /// The command text
    command: String,
}

impl Component for CommandComponent {
    fn render(&self, _buffer: &mut TerminalBuffer) -> Result<()> {
        Ok(())
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: Measurement::Percent(100),
            preferred_height: Measurement::Cell(1),
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: false,
        }
    }
}
