use anyhow::Result;

use crate::event::ReovimEvent;

pub mod command;
pub mod debug;
pub mod gutter;
pub mod status;
pub mod terminal_buffer;
pub mod text;
pub mod tree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
}

impl Cursor {
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
        col >= self.x
            && col < self.x + self.width
            && row >= self.y
            && row < self.y + self.height
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    pub max_width: u16,
    pub max_height: u16,
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
        }
    }
}

pub trait Component {
    /// Render the component to the given terminal buffer
    fn render(&self, buffer: &mut terminal_buffer::TerminalBuffer) -> Result<()>;

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

    /// Return the minimum and maximum scroll offsets allowed for this component
    /// (min_scroll, max_scroll)
    fn scroll_bounds(&self) -> (usize, usize) {
        (0, usize::MAX)
    }

    /// Return the cursor bounds allowed for this component
    /// (min_col, max_col, min_row, max_row)
    fn cursor_bounds(&self, width: u16, height: u16, _formatting: &Formatting) -> (u16, u16, u16, u16) {
        (0, width, 0, height)
    }

    /// Provide default formatting for this component
    fn default_formatting(&self) -> Formatting {
        Formatting::default()
    }

    /// Return child nodes to be automatically added when this component is added to the tree
    ///
    /// # Example
    /// ```ignore
    /// fn child_nodes(&self) -> Vec<tree::ComponentNode<'static>> {
    ///     vec![
    ///         tree::ComponentNode::Status(StatusComponent::new("child1")),
    ///         tree::ComponentNode::Status(StatusComponent::new("child2")),
    ///     ]
    /// }
    /// ```
    fn child_nodes(&self) -> Vec<tree::ComponentNode<'static>> {
        vec![]
    }
}
