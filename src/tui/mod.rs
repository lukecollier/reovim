use std::io::Stdout;

use anyhow::Result;

use crate::event::ReovimEvent;
use crate::tui::status::StatusComponent;

pub mod status;
pub mod window;

pub trait Component {
    fn render(&self, stdout: &mut Stdout) -> Result<()>;

    // is this dumb? We basically subscribe components to certain events?
    fn update(&mut self, event: ReovimEvent) -> Result<()>;
}
