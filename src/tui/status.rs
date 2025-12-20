use std::io::Stdout;

use crate::{event::ReovimEvent, tui::Component};

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    cursor::MoveToNextLine,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, EnterAlternateScreen, LeaveAlternateScreen},
};

pub struct StatusComponent<'a> {
    /// The file name for the bottom left.
    file_name: &'a str,
}

impl<'a> Component for StatusComponent<'a> {
    fn render(&self, stdout: &mut Stdout) -> Result<()> {
        stdout
            .execute(SetBackgroundColor(Color::Reset))?
            .execute(SetBackgroundColor(Color::Black))?
            .execute(SetForegroundColor(Color::Yellow))?
            .execute(Print(self.file_name))?
            .execute(MoveToNextLine(1))?
            .execute(SetBackgroundColor(Color::Reset))?;
        Ok(())
    }

    fn update(&mut self, event: ReovimEvent) -> Result<()> {
        todo!()
    }
}
