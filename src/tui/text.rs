use std::usize;

use crate::{
    event::ReovimEvent,
    tui::{Component, Formatting, Measurement, Overflow, terminal_buffer::TerminalBuffer},
};

use anyhow::Result;
use crossterm::{
    event::{MouseEvent, MouseEventKind},
    style::Color,
};
use tracing::info;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct TextComponent<'a> {
    lines: Vec<&'a str>,
    show_gutter: bool,
    start_at: usize,
}

impl<'a> TextComponent<'a> {
    pub fn new(content: &'a str, start_at: usize) -> Self {
        TextComponent {
            lines: content.lines().collect(),
            show_gutter: true,
            start_at,
        }
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
fn split_by_width(text: &str, max_width: u16) -> Vec<String> {
    let max_width = max_width as usize;
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width + ch_width > max_width && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
            current_width = 0;
        } else {
            current.push(ch);
            current_width += ch_width;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

impl<'a> Component for TextComponent<'a> {
    fn render(&self, buffer: &mut TerminalBuffer) -> Result<()> {
        let render_lines =
            self.start_at..(self.start_at + buffer.height() as usize).min(self.lines.len());
        let mut counts = 0;
        for (i, line) in self.lines[render_lines].iter().enumerate() {
            let new_lines = split_by_width(&line, buffer.width());
            let mut first_line = true;
            for next_line in new_lines {
                if self.show_gutter {
                    let number = i + self.start_at + 1;
                    let number_padding = self.lines.len().to_string().width() as u16;
                    let row_number = pad_or_truncate(&number.to_string(), number_padding);
                    buffer
                        .set_background(Color::Reset)
                        .set_foreground(Color::Yellow);
                    buffer.write(&"│");
                    buffer
                        .set_background(Color::Reset)
                        .set_foreground(Color::DarkGrey);
                    if first_line {
                        first_line = false;
                        buffer.write(&row_number);
                    } else {
                        buffer.write(&" ".repeat(number_padding as usize));
                    }
                    buffer
                        .set_background(Color::Reset)
                        .set_foreground(Color::Reset);
                    buffer.write(&" ");
                };
                buffer.write(&next_line);
                if i < self.lines.len() - 1 {
                    buffer.newline();
                }
            }
            counts += 1;
            if counts >= buffer.height() {
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
                    kind: MouseEventKind::ScrollUp,
                    column: _,
                    row: _,
                    modifiers: _,
                }) => {
                    self.start_at = self.start_at.saturating_sub(1);
                    return Ok(true); // Component changed, needs re-render
                }
                ReovimEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: _,
                    row: _,
                    modifiers: _,
                }) => {
                    self.start_at = (self.start_at + 1).min(self.lines.len() - 1);
                    return Ok(true); // Component changed, needs re-render
                }
                _ => {}
            }
        }
        Ok(false) // No changes
    }

    fn default_formatting(&self) -> Formatting {
        Formatting {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: Measurement::Fill, // Leave room for gutter
            preferred_height: Measurement::Fill,
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: true,
        }
    }
}
