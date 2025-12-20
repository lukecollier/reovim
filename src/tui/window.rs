use std::io::Stdout;

use crate::{event::ReovimEvent, tui::Component};

use anyhow::Result;
use crossterm::ExecutableCommand;

pub struct WindowComponent<'a> {
    /// The file name for the bottom left.
    buffer: &'a str,
}

impl<'a> Component for WindowComponent<'a> {
    fn render(&self, stdout: &mut Stdout) -> Result<()> {
        stdout.execute(crossterm::cursor::MoveTo(0, 0))?;
        Ok(())
    }

    fn update(&mut self, event: ReovimEvent) -> Result<()> {
        todo!()
    }
}
