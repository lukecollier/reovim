use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Debug, Clone)]
pub enum ReovimEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}
