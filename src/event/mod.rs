use anyhow::Result;

use crate::tui::status::StatusComponent;

pub enum WindowEvent {
    Open(),
}

pub enum ReovimEvent {
    Window(WindowEvent),
}
