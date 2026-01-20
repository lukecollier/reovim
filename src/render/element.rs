use anyhow::Result;
use crossterm::style::Color;

pub enum TerminalCommand<'a> {
    Write(&'a str),
    Writeln(&'a str),
    Newline,
    Background(Color),
    Foreground(Color),
}

pub struct TerminalOutput<'a> {
    commands: Vec<TerminalCommand<'a>>,
}

impl<'a> TerminalOutput<'a> {
    fn write(&mut self, str: &'a str) -> &mut Self {
        self.commands.push(TerminalCommand::Write(str));
        self
    }
    fn writeln(&mut self, str: &'a str) -> &mut Self {
        self.commands.push(TerminalCommand::Writeln(str));
        self
    }
    fn newline(&mut self) -> &mut Self {
        self.commands.push(TerminalCommand::Newline);
        self
    }
    fn background(&mut self, color: Color) -> &mut Self {
        self.commands.push(TerminalCommand::Background(color));
        self
    }
    fn foreground(&mut self, color: Color) -> &mut Self {
        self.commands.push(TerminalCommand::Foreground(color));
        self
    }
}

struct Query {}
impl Query {
    fn focused(&self) -> bool {
        todo!()
    }
}

struct Command {}

impl Command {}

struct Formatting {}

impl Default for Formatting {
    fn default() -> Self {
        Self {}
    }
}

impl Formatting {}

pub trait Element {
    fn render(&self, _output: &mut TerminalOutput, _query: &Query);
    fn children(&self, commands: &mut Command);
    fn update(&self, _commands: &mut Command, _query: &Query) -> Result<bool> {
        Ok(false)
    }
    fn default_formatting(&self, _query: &Query) -> Formatting {
        Formatting::default()
    }
}
