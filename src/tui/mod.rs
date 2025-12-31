use anyhow::Result;
use crossterm::cursor::SetCursorStyle;

use crate::event::ReovimEvent;

pub mod command;
pub mod debug;
pub mod editor;
pub mod status;
pub mod terminal_buffer;
pub mod text;
pub mod tree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    VerticalSplit,
    HorizontalSplit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Line,
    Underline,
}

impl CursorStyle {
    fn to_command(self) -> SetCursorStyle {
        match self {
            CursorStyle::Block => SetCursorStyle::SteadyBlock,
            CursorStyle::Line => SetCursorStyle::SteadyBar,
            CursorStyle::Underline => SetCursorStyle::SteadyUnderScore,
        }
    }
}

impl Default for CursorStyle {
    fn default() -> Self {
        CursorStyle::Block
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
}

impl Cursor {
    fn from_xy(row: u16, col: u16) -> Cursor {
        Cursor { row, col }
    }
    fn new() -> Cursor {
        Cursor { row: 0, col: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ComponentQuery {
    focus: bool,
}

impl ComponentQuery {
    fn has_focus(&self) -> bool {
        self.focus
    }
}

impl Rect {
    fn empty() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }

    /// Check if the given column and row are within this rect's bounds
    pub fn contains(&self, col: u16, row: u16) -> bool {
        col >= self.x && col < self.x + self.width && row >= self.y && row < self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    /// Sends new text to the next line after the overflow
    Wrap,
    /// Hides content after the overflow
    Hide,
    /// Content can be scrolled
    Scroll,
}

#[derive(Debug, Clone, Copy)]
pub enum Measurement {
    /// Exact number of cells
    Cell(usize),
    /// Percentage of available space
    Percent(u8),
    /// Size based on rendered content
    Content,
    Fill,
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct Formatting {
    pub preferred_x: Measurement,
    pub preferred_y: Measurement,
    pub preferred_width: Measurement,
    pub preferred_height: Measurement,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub request_focus: bool,
    pub layout_mode: LayoutMode,
    pub focusable: bool,
}

impl Default for Formatting {
    fn default() -> Self {
        Self {
            preferred_x: Measurement::Cell(0),
            preferred_y: Measurement::Cell(0),
            preferred_width: Measurement::Percent(100),
            preferred_height: Measurement::Percent(100),
            overflow_x: Overflow::Hide,
            overflow_y: Overflow::Hide,
            request_focus: false,
            layout_mode: LayoutMode::VerticalSplit,
            focusable: true,
        }
    }
}

pub trait Component {
    /// Render the component to the given terminal buffer
    fn render(
        &self,
        _buffer: &mut terminal_buffer::TerminalBuffer,
        _query: ComponentQuery,
    ) -> Result<()> {
        Ok(())
    }

    /// Handle an event with controlled access to the component tree
    ///
    /// # Parameters
    /// - `event` - the event to handle
    /// - `commands` - interface for querying and modifying the tree (limited API)
    ///
    /// # Returns
    /// A bool indicating whether the component changed and needs to be re-rendered
    ///
    /// # Example
    /// ```ignore
    /// fn update(&mut self, event: ReovimEvent, commands: &mut tree::ComponentCommands) -> Result<bool> {
    ///     if let Some(child_ids) = commands.children() {
    ///         // Access children safely
    ///     }
    ///     Ok(false)  // No changes, don't need to re-render
    /// }
    /// ```
    fn update(
        &mut self,
        _event: ReovimEvent,
        _commands: &mut tree::ComponentCommands,
    ) -> Result<bool> {
        Ok(false)
    }

    /// Provide default formatting for this component
    fn default_formatting(&self) -> Formatting {
        Formatting::default()
    }

    /// Initialize child components for this component
    /// Called after the component is added to the tree, allows the component to add children
    /// through the provided ComponentCommands
    fn children(&mut self, _commands: &mut tree::ComponentCommands) -> Result<()> {
        Ok(())
    }
}
