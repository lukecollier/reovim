use crossterm::{style::Color, terminal::Clear};

#[derive(Clone, Debug)]
pub enum TerminalCommand {
    Print(char),
    Newline,
    SetForeground(Color),
    SetBackground(Color),
    Clear,
}

/// A virtual buffer for rendering to a bounded rectangular area
/// Components render into this instead of directly to stdout
pub struct TerminalBuffer {
    buffer: Vec<TerminalCommand>,
    width: u16,
    height: u16,
    cursor: Option<(u16, u16)>,
    has_focus: bool,
}

impl TerminalBuffer {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            buffer: Vec::new(),
            width,
            height,
            cursor: None,
            has_focus: false,
        }
    }

    pub fn writeln(&mut self, text: &str) -> &mut Self {
        self.write(text);
        self.newline()
    }

    pub fn write(&mut self, text: &str) -> &mut Self {
        for ch in text.chars() {
            self.buffer.push(TerminalCommand::Print(ch));
        }
        self
    }

    pub fn newline(&mut self) -> &mut Self {
        self.buffer.push(TerminalCommand::Newline);
        self
    }

    pub fn set_foreground(&mut self, color: Color) -> &mut Self {
        self.buffer.push(TerminalCommand::SetForeground(color));
        self
    }

    pub fn set_background(&mut self, color: Color) -> &mut Self {
        self.buffer.push(TerminalCommand::SetBackground(color));
        self
    }

    pub fn clear(&mut self) -> &mut Self {
        self.buffer.push(TerminalCommand::Clear);
        self
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn commands(&self) -> &[TerminalCommand] {
        &self.buffer
    }

    pub fn set_cursor(&mut self, x: u16, y: u16) -> &mut Self {
        self.cursor = Some((x, y));
        self
    }

    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor
    }

    pub fn clear_cursor(&mut self) -> &mut Self {
        self.cursor = None;
        self
    }

    pub fn has_focus(&self) -> bool {
        self.has_focus
    }

    pub fn set_focus(&mut self, focused: bool) -> &mut Self {
        self.has_focus = focused;
        self
    }
}
