use crate::event::ReovimEvent;
use crate::tui::debug::DebugComponent;
use crate::tui::status::StatusComponent;
use crate::tui::terminal_buffer::{TerminalBuffer, TerminalCommand};
use crate::tui::text::TextComponent;
use crate::tui::{
    Component, Cursor, CursorStyle, Formatting, LayoutMode, Measurement, Overflow, Rect,
};
use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{MouseButton, MouseEventKind};
use crossterm::style::{Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use std::io::Stdout;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub type ComponentId = usize;

/// Split text into chunks of max width, respecting unicode character widths
fn split_by_width(text: &str, max_width: u16) -> Vec<&str> {
    let max_width = max_width as usize;
    let mut chunks = Vec::new();
    let mut current_width = 0;
    let mut start_byte = 0;

    for (byte_pos, ch) in text.char_indices() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width + ch_width > max_width && start_byte < byte_pos {
            // Exceeded width, slice from start_byte to byte_pos
            chunks.push(&text[start_byte..byte_pos]);
            start_byte = byte_pos;
            current_width = ch_width;
        } else {
            current_width += ch_width;
        }
    }

    // Add remaining text
    if start_byte < text.len() {
        chunks.push(&text[start_byte..]);
    }

    chunks
}

/// Commands that a component can perform on the tree
/// This provides a limited interface to prevent arbitrary tree mutations
pub struct ComponentCommands<'a> {
    tree: &'a mut ComponentTree<'a>,
    self_id: ComponentId,
}

impl<'a> ComponentCommands<'a> {
    pub fn new(tree: &'a mut ComponentTree<'a>, self_id: ComponentId) -> Self {
        Self { tree, self_id }
    }

    pub fn has_focus(&self) -> bool {
        // A component has focus if it's in the focus path (including ancestors of the focused leaf)
        self.tree.focus_path.contains(&self.self_id)
    }

    /// Get the IDs of this component's children
    pub fn children(&self) -> Option<Vec<ComponentId>> {
        self.tree.children(self.self_id)
    }

    /// Set horizontal scroll offset
    pub fn set_scroll_x(&mut self, offset: usize) {
        if let Some(scroll) = self.tree.scroll_x.get_mut(self.self_id) {
            *scroll = offset;
        }
    }

    /// Set vertical scroll offset
    pub fn set_scroll_y(&mut self, offset: usize) {
        if let Some(scroll) = self.tree.scroll_y.get_mut(self.self_id) {
            *scroll = offset;
        }
    }

    /// Get the current horizontal scroll offset
    pub fn get_scroll_x(&self) -> usize {
        self.tree.scroll_x.get(self.self_id).copied().unwrap_or(0)
    }

    /// Get the current vertical scroll offset
    pub fn get_scroll_y(&self) -> usize {
        self.tree.scroll_y.get(self.self_id).copied().unwrap_or(0)
    }

    /// Convert global screen coordinates to local component coordinates
    /// This accounts for the component's rect position and scroll offset
    /// Result is the cursor position within the component's content
    pub fn global_to_local(&self, global_col: u16, global_row: u16) -> (u16, u16) {
        let rect = self
            .tree
            .rects
            .get(self.self_id)
            .copied()
            .unwrap_or_default();
        let scroll_x = self.tree.scroll_x.get(self.self_id).copied().unwrap_or(0);
        let scroll_y = self.tree.scroll_y.get(self.self_id).copied().unwrap_or(0);

        // Subtract rect offset to get local rect coordinates
        let local_col = global_col.saturating_sub(rect.x);
        let local_row = global_row.saturating_sub(rect.y);

        // Add scroll offset to get position in the component's content
        let cursor_col = (local_col as usize + scroll_x) as u16;
        let cursor_row = (local_row as usize + scroll_y) as u16;

        (cursor_col, cursor_row)
    }

    /// Convert local component coordinates to global screen coordinates
    /// This accounts for the component's rect position and scroll offset
    pub fn local_to_global(&self, local_col: u16, local_row: u16) -> (u16, u16) {
        let rect = self
            .tree
            .rects
            .get(self.self_id)
            .copied()
            .unwrap_or_default();
        let scroll_x = self.tree.scroll_x.get(self.self_id).copied().unwrap_or(0);
        let scroll_y = self.tree.scroll_y.get(self.self_id).copied().unwrap_or(0);

        // Subtract scroll offset to get position within visible area
        let visible_col = (local_col as usize).saturating_sub(scroll_x) as u16;
        let visible_row = (local_row as usize).saturating_sub(scroll_y) as u16;

        // Add rect offset to get global screen coordinates
        let global_col = visible_col.saturating_add(rect.x);
        let global_row = visible_row.saturating_add(rect.y);

        (global_col, global_row)
    }

    /// Set cursor position (col, row) for the component
    pub fn clamp_cursor_col(&mut self, min_col: u16, max_col: u16) {
        if let Some(col_slot) = self.tree.cursor_col.get_mut(self.self_id) {
            *col_slot = (*col_slot).clamp(min_col, max_col);
        }
        self.tree.mark_dirty(self.self_id);
    }

    /// Set cursor position (col, row) for the component
    pub fn set_cursor(&mut self, col: u16, row: u16) {
        // Get formatting and cursor bounds from the component
        let formatting = self
            .tree
            .formatting
            .get(self.self_id)
            .copied()
            .unwrap_or_default();

        // Measure logical bounds
        let (logical_width, logical_height) = self.tree.measure_logical_bounds(self.self_id);

        let min_col = 0;
        let max_col = logical_width.saturating_sub(1);
        let min_row = 0;
        let max_row = logical_height.saturating_sub(1);

        let clamped_col = col.max(min_col).min(max_col);
        let clamped_row = row.max(min_row).min(max_row);

        if let Some(col_slot) = self.tree.cursor_col.get_mut(self.self_id) {
            *col_slot = clamped_col;
        }
        if let Some(row_slot) = self.tree.cursor_row.get_mut(self.self_id) {
            *row_slot = clamped_row;
        }
        self.tree.mark_dirty(self.self_id);
    }

    /// Move cursor by the given offset (col_delta, row_delta)
    /// Cursors are stored relative to each component
    /// Navigation is simple: move within bounds, hit boundary -> navigate siblings
    pub fn move_cursor(&mut self, col_delta: i32, row_delta: i32) {
        // Get the deepest focused component
        let current_id = if !self.tree.focus_path.is_empty() {
            *self.tree.focus_path.last().unwrap()
        } else {
            self.self_id
        };

        let formatting = self
            .tree
            .formatting
            .get(current_id)
            .copied()
            .unwrap_or_default();

        // Get current cursor position
        let current_col = self.tree.cursor_col.get(current_id).copied().unwrap_or(0);
        let current_row = self.tree.cursor_row.get(current_id).copied().unwrap_or(0);

        // Measure logical (unwrapped) bounds for cursor navigation
        let (logical_width, logical_height) = self.tree.measure_logical_bounds(current_id);

        let min_col = 0;
        let max_col = logical_width.saturating_sub(1);
        let min_row = 0;
        let mut max_row = logical_height.saturating_sub(1);

        // For wrapped components, treat as single logical row
        if formatting.overflow_x == Overflow::Wrap {
            max_row = 0;
        }

        // Try to move within current component
        let new_col = if col_delta > 0 {
            current_col.saturating_add(col_delta as u16)
        } else if col_delta < 0 {
            current_col.saturating_sub((-col_delta) as u16)
        } else {
            current_col
        };

        let new_row = if row_delta > 0 {
            current_row.saturating_add(row_delta as u16)
        } else if row_delta < 0 {
            current_row.saturating_sub((-row_delta) as u16)
        } else {
            current_row
        };

        // Detect if saturation occurred (cursor didn't move in the requested direction)
        let trying_to_move_up_but_stayed = row_delta < 0 && new_row == current_row;
        let trying_to_move_down_but_stayed = row_delta > 0 && new_row == current_row;
        let trying_to_move_left_but_stayed = col_delta < 0 && new_col == current_col;
        let trying_to_move_right_but_stayed = col_delta > 0 && new_col == current_col;
        let at_boundary = trying_to_move_up_but_stayed
            || trying_to_move_down_but_stayed
            || trying_to_move_left_but_stayed
            || trying_to_move_right_but_stayed;

        // Check if new position is within bounds AND not at a boundary trying to go past it
        if !at_boundary
            && new_col >= min_col
            && new_col <= max_col
            && new_row >= min_row
            && new_row <= max_row
        {
            // Move is valid within component
            if let Some(col_slot) = self.tree.cursor_col.get_mut(current_id) {
                *col_slot = new_col;
            }
            if let Some(row_slot) = self.tree.cursor_row.get_mut(current_id) {
                *row_slot = new_row;
            }
            self.tree.mark_dirty(current_id);
            return;
        }

        // At boundary - backtrack up the tree to find a parent that supports navigation in this direction
        let mut backtrack_id = current_id;
        while let Some(parent) = self.tree.parent(backtrack_id).and_then(|p| p) {
            let parent_layout = self
                .tree
                .formatting
                .get(parent)
                .copied()
                .unwrap_or_default()
                .layout_mode;

            // Check if this navigation direction makes sense for parent's layout
            let should_navigate_siblings = matches!(
                (parent_layout, col_delta != 0, row_delta != 0),
                (LayoutMode::VerticalSplit, false, true) | // Up/Down in VerticalSplit
                (LayoutMode::HorizontalSplit, true, false) // Left/Right in HorizontalSplit
            );

            if should_navigate_siblings {
                // Update parent's cursor_col to current position so next sibling knows which column to use
                if let Some(col_slot) = self.tree.cursor_col.get_mut(parent) {
                    *col_slot = current_col;
                }
                self.navigate_siblings(parent, col_delta, row_delta);
                return;
            }

            // Parent doesn't support this navigation direction, keep backtracking
            backtrack_id = parent;
        }

        // Clamp to boundaries and stay in component
        let clamped_col = new_col.max(min_col).min(max_col);
        let clamped_row = new_row.max(min_row).min(max_row);

        if let Some(col_slot) = self.tree.cursor_col.get_mut(current_id) {
            *col_slot = clamped_col;
        }
        if let Some(row_slot) = self.tree.cursor_row.get_mut(current_id) {
            *row_slot = clamped_row;
        }
        self.tree.mark_dirty(current_id);
    }

    /// Set focus to a component and automatically descend to the first focusable child leaf
    fn set_focus_with_descent(&mut self, component_id: ComponentId) {
        // Find the deepest focusable descendant (automatically descends into first focusable child)
        let focus_id = self.find_focusable_descendant(component_id);

        // Update focus
        self.self_id = focus_id;
        self.tree.focus = focus_id;
        self.tree.focus_path = self.tree.build_focus_path(focus_id);
        self.tree.mark_dirty(focus_id);
    }

    /// Set focus to a component and automatically descend to the last focusable child leaf
    fn set_focus_with_descent_to_last(&mut self, component_id: ComponentId) {
        // Find the deepest focusable descendant (automatically descends into last focusable child)
        let focus_id = self.find_focusable_descendant_last(component_id);

        // Update focus
        self.self_id = focus_id;
        self.tree.focus = focus_id;
        self.tree.focus_path = self.tree.build_focus_path(focus_id);
        self.tree.mark_dirty(focus_id);
    }

    /// Navigate to next/previous sibling component
    /// Updates parent's cursor_row to track which child is focused
    fn navigate_siblings(&mut self, parent_id: ComponentId, col_delta: i32, row_delta: i32) {
        if let Some(siblings) = self.tree.children(parent_id) {
            if siblings.is_empty() {
                return;
            }

            // Parent's cursor_row tracks which child is currently focused
            let parent_col = self.tree.cursor_col.get(parent_id).copied().unwrap_or(0);
            let parent_row = self.tree.cursor_row.get(parent_id).copied().unwrap_or(0);

            // Determine next sibling based on movement direction
            let current_pos = (parent_row as usize).min(siblings.len() - 1);
            let next_pos = if row_delta > 0 || col_delta > 0 {
                (current_pos + 1).min(siblings.len() - 1)
            } else if row_delta < 0 || col_delta < 0 {
                current_pos.saturating_sub(1)
            } else {
                return;
            };

            // Only move if position changed
            if next_pos != current_pos {
                let next_id = siblings[next_pos];

                // Update parent's cursor to point to new child
                if let Some(row_slot) = self.tree.cursor_row.get_mut(parent_id) {
                    *row_slot = next_pos as u16;
                }

                // Try to focus the new component with automatic descent to first or last focusable child
                if self.try_enter_component(next_id) {
                    let is_moving_up = row_delta < 0 || col_delta < 0;

                    if is_moving_up {
                        // Moving up: descend to last focusable child
                        self.set_focus_with_descent_to_last(next_id);
                    } else {
                        // Moving down: descend to first focusable child
                        self.set_focus_with_descent(next_id);
                    }

                    // Get the focused component after descent
                    let focus_id = self.tree.focus;
                    let (logical_width, logical_height) =
                        self.tree.measure_logical_bounds(focus_id);

                    let max_col = logical_width.saturating_sub(1);
                    let max_row = logical_height.saturating_sub(1);

                    // Clamp column to child's total content width
                    let clamped_col = parent_col.min(max_col);

                    // Set cursor to logical bounds, not visual wrapped position
                    // Wrapping is only a rendering concern, not a navigation concern
                    let target_row = if is_moving_up {
                        // When moving up: position at the last logical row
                        max_row
                    } else {
                        // When moving down: position at the first logical row
                        0
                    };

                    // Set cursor position
                    if let Some(col_slot) = self.tree.cursor_col.get_mut(focus_id) {
                        *col_slot = clamped_col;
                    }
                    if let Some(row_slot) = self.tree.cursor_row.get_mut(focus_id) {
                        *row_slot = target_row;
                    }

                    self.tree.mark_dirty(parent_id);
                }
            }
        }
    }

    /// Try to enter a component, descending into focusable descendants if needed
    fn try_enter_component(&self, component_id: ComponentId) -> bool {
        let formatting = self
            .tree
            .formatting
            .get(component_id)
            .copied()
            .unwrap_or_default();

        if formatting.focusable {
            return true;
        }

        // If not focusable, try to find a focusable child
        if let Some(children) = self.tree.children(component_id) {
            for &child_id in &children {
                if self.try_enter_component(child_id) {
                    return true;
                }
            }
        }

        false
    }

    /// Find the actual descendant component to focus when entering a component
    /// If the component is focusable with focusable children, returns the first focusable child's descendant
    /// If the component is focusable with no focusable children, returns the component itself
    fn find_focusable_descendant(&self, component_id: ComponentId) -> ComponentId {
        let formatting = self
            .tree
            .formatting
            .get(component_id)
            .copied()
            .unwrap_or_default();

        // If this component is focusable, check if it has focusable children to descend into
        if formatting.focusable {
            if let Some(children) = self.tree.children(component_id) {
                // Try to find the first focusable child
                for &child_id in &children {
                    if self.try_enter_component(child_id) {
                        // Recursively find the deepest focusable descendant of this child
                        return self.find_focusable_descendant(child_id);
                    }
                }
            }
            // No focusable children, return this component
            return component_id;
        }

        // If not focusable, try to find a focusable descendant
        if let Some(children) = self.tree.children(component_id) {
            for &child_id in &children {
                if self.try_enter_component(child_id) {
                    return self.find_focusable_descendant(child_id);
                }
            }
        }

        // Shouldn't reach here if try_enter_component was already called
        component_id
    }

    /// Find the actual descendant component to focus when entering a component from below
    /// Like find_focusable_descendant but descends into the last focusable child instead of first
    fn find_focusable_descendant_last(&self, component_id: ComponentId) -> ComponentId {
        let formatting = self
            .tree
            .formatting
            .get(component_id)
            .copied()
            .unwrap_or_default();

        // If this component is focusable, check if it has focusable children to descend into
        if formatting.focusable {
            if let Some(children) = self.tree.children(component_id) {
                // Try to find the last focusable child (iterate in reverse)
                for &child_id in children.iter().rev() {
                    if self.try_enter_component(child_id) {
                        // Recursively find the deepest focusable descendant of this child
                        return self.find_focusable_descendant_last(child_id);
                    }
                }
            }
            // No focusable children, return this component
            return component_id;
        }

        // If not focusable, try to find a focusable descendant (iterate in reverse)
        if let Some(children) = self.tree.children(component_id) {
            for &child_id in children.iter().rev() {
                if self.try_enter_component(child_id) {
                    return self.find_focusable_descendant_last(child_id);
                }
            }
        }

        // Shouldn't reach here if try_enter_component was already called
        component_id
    }

    /// Get the cursor position, accounting for scroll offset (returns localized coordinates)
    pub fn get_cursor(&self) -> Cursor {
        let col = self.tree.cursor_col.get(self.self_id).copied().unwrap_or(0);
        let row = self.tree.cursor_row.get(self.self_id).copied().unwrap_or(0);
        let scroll_x = self.tree.scroll_x.get(self.self_id).copied().unwrap_or(0);
        let scroll_y = self.tree.scroll_y.get(self.self_id).copied().unwrap_or(0);

        // Convert absolute cursor position to relative position within visible area
        let relative_col = (col as usize).saturating_sub(scroll_x) as u16;
        let relative_row = (row as usize).saturating_sub(scroll_y) as u16;

        Cursor::from_xy(relative_row, relative_col)
    }

    /// Set the cursor display style for this component
    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        if let Some(style_slot) = self.tree.cursor_style.get_mut(self.self_id) {
            *style_slot = style;
        }
    }

    /// Get the current cursor display style for this component
    pub fn get_cursor_style(&self) -> CursorStyle {
        self.tree
            .cursor_style
            .get(self.self_id)
            .copied()
            .unwrap_or_default()
    }

    /// Add a child component with default formatting
    pub fn add_child(&mut self, child: ComponentNode<'a>) -> Result<ComponentId> {
        self.tree.add_child(self.self_id, child)
    }

    /// Add a child component with custom formatting
    pub fn add_child_with_formatting(
        &mut self,
        child: ComponentNode<'a>,
        formatting: Formatting,
    ) -> Result<ComponentId> {
        self.tree
            .add_child_with_formatting(self.self_id, child, formatting)
    }

    /// Add a component as a child with default formatting
    pub fn add_component<C: Component + 'static>(&mut self, component: C) -> Result<ComponentId> {
        let boxed: Box<dyn Component> = Box::new(component);
        self.add_child(ComponentNode::Component(boxed))
    }
}

/// A frame is a layout container that holds children
pub struct Frame {
    pub layout_mode: LayoutMode,
}

impl Frame {
    pub fn new(layout_mode: LayoutMode) -> Self {
        Self { layout_mode }
    }
}

/// All possible component types in the arena
pub enum ComponentNode<'a> {
    Frame(Frame),
    Status(StatusComponent),
    Text(TextComponent<'a>),
    Debug(DebugComponent),
    Component(Box<dyn Component>),
}

impl<'a> ComponentNode<'a> {
    pub fn render(&self, buffer: &mut TerminalBuffer) -> Result<()> {
        match self {
            ComponentNode::Frame(_) => {
                // Frames don't render themselves, only their children
                Ok(())
            }
            ComponentNode::Status(status_component) => status_component.render(buffer),
            ComponentNode::Text(text_component) => text_component.render(buffer),
            ComponentNode::Debug(component) => component.render(buffer),
            ComponentNode::Component(component) => component.render(buffer),
        }
    }

    pub fn update(
        &mut self,
        event: ReovimEvent,
        commands: &mut ComponentCommands<'a>,
    ) -> Result<bool> {
        match self {
            ComponentNode::Frame(_) => Ok(false),
            ComponentNode::Status(status_component) => status_component.update(event, commands),
            ComponentNode::Text(text_component) => text_component.update(event, commands),
            ComponentNode::Debug(component) => component.update(event, commands),
            ComponentNode::Component(component) => component.update(event, commands),
        }
    }

    pub fn initialize_children(&mut self, commands: &mut ComponentCommands<'a>) -> Result<()> {
        match self {
            ComponentNode::Frame(_) => Ok(()),
            ComponentNode::Status(status_component) => status_component.children(commands),
            ComponentNode::Text(text_component) => text_component.children(commands),
            ComponentNode::Debug(component) => component.children(commands),
            ComponentNode::Component(component) => component.children(commands),
        }
    }
}

/// Arena-based component tree
/// Stores all components in a flat vector and references them by index
pub struct ComponentTree<'a> {
    components: Vec<ComponentNode<'a>>,
    /// parent[i] is the parent of component i
    parent: Vec<Option<ComponentId>>,
    /// children[i] is the list of child component IDs for component i
    children: Vec<Vec<ComponentId>>,
    /// rects[i] is the position and size of component i
    rects: Vec<Rect>,
    /// formatting[i] is the layout preferences for component i
    formatting: Vec<Formatting>,
    /// Which component has focus
    focus: ComponentId,
    /// Root component ID
    root: ComponentId,
    /// Components that have had state change
    dirty: Vec<ComponentId>,
    /// Components that need their children() method called
    pending_initialization: Vec<ComponentId>,
    /// Cursor position for the focused component (if any)
    cursor_pos: Option<(u16, u16)>,
    /// scroll_x[i] is the horizontal scroll offset for component i
    scroll_x: Vec<usize>,
    /// scroll_y[i] is the vertical scroll offset for component i
    scroll_y: Vec<usize>,
    /// cursor_col[i] is the cursor column for component i
    cursor_col: Vec<u16>,
    /// cursor_row[i] is the cursor row for component i
    cursor_row: Vec<u16>,
    /// cursor_initialized[i] tracks whether cursor has been set to minimum bounds for component i
    cursor_initialized: Vec<bool>,
    /// cursor_style[i] is the display style for the cursor of component i
    cursor_style: Vec<CursorStyle>,
    /// Path of component IDs from root to currently focused component
    focus_path: Vec<ComponentId>,
}

impl<'a> ComponentTree<'a> {
    pub fn new(root: ComponentNode<'a>) -> Self {
        Self {
            components: vec![root],
            focus: 0,
            parent: vec![None],
            children: vec![Vec::new()],
            rects: vec![Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            }],
            formatting: vec![Formatting::default()],
            root: 0,
            dirty: vec![0],
            pending_initialization: vec![0],
            cursor_pos: None,
            scroll_x: vec![0],
            scroll_y: vec![0],
            cursor_col: vec![0],
            cursor_row: vec![0],
            cursor_initialized: vec![false],
            cursor_style: vec![CursorStyle::default()],
            focus_path: vec![0], // Start with root in focus path
        }
    }

    /// Add a component as a child of a parent with the component's default formatting
    pub fn add_child(
        &mut self,
        parent_id: ComponentId,
        child: ComponentNode<'a>,
    ) -> Result<ComponentId> {
        let formatting = match &child {
            ComponentNode::Frame(_) => Formatting::default(),
            ComponentNode::Status(component) => component.default_formatting(),
            ComponentNode::Text(component) => component.default_formatting(),
            ComponentNode::Debug(component) => component.default_formatting(),
            ComponentNode::Component(component) => component.default_formatting(),
        };
        self.add_child_with_formatting(parent_id, child, formatting)
    }

    /// Add a component as a child of a parent with custom formatting
    pub fn add_child_with_formatting(
        &mut self,
        parent_id: ComponentId,
        child: ComponentNode<'a>,
        formatting: Formatting,
    ) -> Result<ComponentId> {
        if parent_id >= self.components.len() {
            return Err(anyhow::anyhow!("Parent component not found"));
        }

        let child_id = self.components.len();

        // Check if this is a Frame before moving it
        let is_frame = matches!(child, ComponentNode::Frame(_));

        self.components.push(child);
        self.parent.push(Some(parent_id));
        self.rects.push(Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        self.formatting.push(formatting);
        self.scroll_x.push(0);
        self.scroll_y.push(0);
        self.children.push(Vec::new()); // Initialize empty children list for this component
        // Cursor will be initialized to minimum bounds after first layout
        self.cursor_col.push(0);
        self.cursor_row.push(0);
        self.cursor_initialized.push(false);
        self.cursor_style.push(CursorStyle::default());

        // Mark as dirty so it renders on first frame (but not Frames, which shouldn't clear)
        if !is_frame {
            self.mark_dirty(child_id);
        }

        // Set focus if this component requests it
        if formatting.request_focus {
            self.focus = child_id;
        }

        // Add child to parent's children list (using tree storage, not component storage)
        if let Some(children) = self.children.get_mut(parent_id) {
            children.push(child_id);
        }

        // Mark component for initialization - will call children() method later
        if !is_frame {
            self.pending_initialization.push(child_id);
        }

        Ok(child_id)
    }

    /// Get a component by ID (immutable)
    pub fn get(&self, id: ComponentId) -> Option<&ComponentNode<'a>> {
        self.components.get(id)
    }

    /// Get a component by ID (mutable)
    pub fn get_mut(&mut self, id: ComponentId) -> Option<&mut ComponentNode<'a>> {
        self.components.get_mut(id)
    }

    /// Get children of a component
    pub fn children(&self, id: ComponentId) -> Option<Vec<ComponentId>> {
        self.children.get(id).map(|c| c.clone())
    }

    /// Get parent of a component
    pub fn parent(&self, id: ComponentId) -> Option<Option<ComponentId>> {
        self.parent.get(id).copied()
    }

    /// Get the bounding rectangle of a component
    pub fn rect(&self, id: ComponentId) -> Option<Rect> {
        self.rects.get(id).copied()
    }

    /// Convert global screen coordinates to local component coordinates
    /// This accounts for the component's rect position and scroll offset
    /// Result is the cursor position within the component's content
    pub fn global_to_local(&self, id: ComponentId, global_col: u16, global_row: u16) -> (u16, u16) {
        let rect = self.rects.get(id).copied().unwrap_or_default();
        let scroll_x = self.scroll_x.get(id).copied().unwrap_or(0);
        let scroll_y = self.scroll_y.get(id).copied().unwrap_or(0);

        // Subtract rect offset to get local rect coordinates
        let local_col = global_col.saturating_sub(rect.x);
        let local_row = global_row.saturating_sub(rect.y);

        // Add scroll offset to get position in the component's content
        let cursor_col = (local_col as usize + scroll_x) as u16;
        let cursor_row = (local_row as usize + scroll_y) as u16;

        (cursor_col, cursor_row)
    }

    /// Convert local component coordinates to global screen coordinates
    /// This accounts for the component's rect position and scroll offset
    pub fn local_to_global(&self, id: ComponentId, local_col: u16, local_row: u16) -> (u16, u16) {
        let rect = self.rects.get(id).copied().unwrap_or_default();
        let scroll_x = self.scroll_x.get(id).copied().unwrap_or(0);
        let scroll_y = self.scroll_y.get(id).copied().unwrap_or(0);

        // Subtract scroll offset to get position within visible area
        let visible_col = (local_col as usize).saturating_sub(scroll_x) as u16;
        let visible_row = (local_row as usize).saturating_sub(scroll_y) as u16;

        // Add rect offset to get global screen coordinates
        let global_col = visible_col.saturating_add(rect.x);
        let global_row = visible_row.saturating_add(rect.y);

        (global_col, global_row)
    }

    /// Calculate actual width/height from a Measurement given available space
    /// Note: Fill returns the full available space; caller should handle distributing Fill space
    fn calculate_size(&self, measurement: Measurement, available: u16) -> u16 {
        match measurement {
            Measurement::Cell(cells) => cells.min(available as usize) as u16,
            Measurement::Percent(percent) => {
                let percent = percent.min(100) as u16;
                (available * percent) / 100
            }
            Measurement::Content => available, // Will be handled specially in layout_children
            Measurement::Fill => available,
        }
    }

    /// Measure the size of a component by rendering it to a temporary buffer
    /// For containers with children, measures the combined child dimensions instead
    fn measure_component(&self, id: ComponentId, max_width: u16, max_height: u16) -> (u16, u16) {
        // If this component has children, measure their combined size instead
        if let Some(child_ids) = self.children(id) {
            if !child_ids.is_empty() {
                let formatting = self.formatting.get(id).copied().unwrap_or_default();

                return match formatting.layout_mode {
                    LayoutMode::VerticalSplit => {
                        // Children are stacked vertically: width is max, height is sum
                        let mut total_height = 0u16;
                        let mut max_width_child = 0u16;

                        for &child_id in &child_ids {
                            let (child_width, child_height) =
                                self.measure_component(child_id, max_width, max_height);
                            max_width_child = max_width_child.max(child_width);
                            total_height = total_height.saturating_add(child_height);
                        }

                        (max_width_child, total_height)
                    }
                    LayoutMode::HorizontalSplit => {
                        // Children are laid out horizontally: width is sum, height is max
                        let mut total_width = 0u16;
                        let mut max_height_child = 0u16;

                        for &child_id in &child_ids {
                            let (child_width, child_height) =
                                self.measure_component(child_id, max_width, max_height);
                            total_width = total_width.saturating_add(child_width);
                            max_height_child = max_height_child.max(child_height);
                        }

                        (total_width, max_height_child)
                    }
                };
            }
        }

        // Leaf component: render it to measure
        let mut buffer = TerminalBuffer::new(max_width, max_height);

        if let Some(component) = self.components.get(id) {
            let _ = component.render(&mut buffer);
            buffer.measure_content()
        } else {
            (0, 0)
        }
    }

    /// Measure logical (unwrapped) bounds of a component
    /// Renders to a very wide buffer to determine content dimensions without wrapping effects
    fn measure_logical_bounds(&self, id: ComponentId) -> (u16, u16) {
        // If this component has children, calculate logical bounds from children
        if let Some(child_ids) = self.children(id) {
            if !child_ids.is_empty() {
                let formatting = self.formatting.get(id).copied().unwrap_or_default();

                return match formatting.layout_mode {
                    LayoutMode::VerticalSplit => {
                        // Children are stacked vertically: width is max, height is sum
                        let mut total_height = 0u16;
                        let mut max_width_child = 0u16;

                        for &child_id in &child_ids {
                            let (child_width, child_height) = self.measure_logical_bounds(child_id);
                            max_width_child = max_width_child.max(child_width);
                            total_height = total_height.saturating_add(child_height);
                        }

                        (max_width_child, total_height)
                    }
                    LayoutMode::HorizontalSplit => {
                        // Children are laid out horizontally: width is sum, height is max
                        let mut total_width = 0u16;
                        let mut max_height_child = 0u16;

                        for &child_id in &child_ids {
                            let (child_width, child_height) = self.measure_logical_bounds(child_id);
                            total_width = total_width.saturating_add(child_width);
                            max_height_child = max_height_child.max(child_height);
                        }

                        (total_width, max_height_child)
                    }
                };
            }
        }

        // Leaf component: render to very wide buffer to get logical dimensions without wrapping
        let mut buffer = TerminalBuffer::new(u16::MAX, u16::MAX);

        if let Some(component) = self.components.get(id) {
            let _ = component.render(&mut buffer);
            buffer.measure_content()
        } else {
            (0, 0)
        }
    }

    /// Calculate the visual (wrapped) height of a component at a given render width
    /// This tells us how many screen rows the component occupies when rendered
    fn get_visual_height(&self, id: ComponentId, render_width: u16) -> u16 {
        let mut buffer = TerminalBuffer::new(render_width, u16::MAX);

        if let Some(component) = self.components.get(id) {
            let _ = component.render(&mut buffer);

            // Count visual rows by tracking when we exceed render_width
            let mut visual_height = 1u16;
            let mut current_x = 0u16;

            for cmd in buffer.commands() {
                match cmd {
                    TerminalCommand::Print(_ch) => {
                        current_x += 1;
                        if current_x >= render_width {
                            visual_height += 1;
                            current_x = 0;
                        }
                    }
                    TerminalCommand::Newline => {
                        visual_height += 1;
                        current_x = 0;
                    }
                    _ => {}
                }
            }

            visual_height
        } else {
            1
        }
    }

    /// Get the actual width of a specific row when the component is rendered at the given width
    /// Used for determining which wrapped row a logical column falls into
    fn get_row_width(&self, id: ComponentId, row: u16, render_width: u16) -> u16 {
        // Render the component to extract its text content
        let mut buffer = TerminalBuffer::new(render_width, u16::MAX);

        if let Some(component) = self.components.get(id) {
            let _ = component.render(&mut buffer);

            // Extract text from buffer commands
            let mut text = String::new();
            for cmd in buffer.commands() {
                match cmd {
                    TerminalCommand::Print(ch) => text.push(*ch),
                    TerminalCommand::Newline => text.push('\n'),
                    _ => {}
                }
            }

            // Split the text by width and return the width of the specified row
            let rows = split_by_width(&text, render_width);
            if let Some(row_content) = rows.get(row as usize) {
                row_content.width() as u16
            } else {
                render_width
            }
        } else {
            render_width
        }
    }

    /// Build the focus path from root to a given component
    /// Returns a vector of component IDs from root to the target component
    fn build_focus_path(&self, target_id: ComponentId) -> Vec<ComponentId> {
        let mut path = vec![target_id];
        let mut current_id = target_id;

        // Walk up the tree from target to root
        while let Some(&Some(parent_id)) = self.parent.get(current_id) {
            path.push(parent_id);
            current_id = parent_id;
        }

        // Reverse to get root-to-target order
        path.reverse();
        path
    }

    /// Calculate the full content bounds of a component including all its children
    /// Returns (full_width, full_height) - the rightmost and bottommost extents of all children
    /// Falls back to the component's own rect dimensions if it has no children
    fn get_content_bounds(&self, id: ComponentId) -> (u16, u16) {
        let rect = self.rect(id).unwrap_or_default();

        // Get children and calculate their extents
        if let Some(child_ids) = self.children(id) {
            if !child_ids.is_empty() {
                let mut max_x = 0u16;
                let mut max_y = 0u16;

                for child_id in child_ids {
                    if let Some(child_rect) = self.rects.get(child_id) {
                        let right_edge = child_rect.x.saturating_add(child_rect.width);
                        let bottom_edge = child_rect.y.saturating_add(child_rect.height);

                        max_x = max_x.max(right_edge);
                        max_y = max_y.max(bottom_edge);
                    }
                }

                // Return the full content dimensions
                return (max_x, max_y);
            }
        }

        // No children, use the component's own dimensions
        (rect.width, rect.height)
    }

    pub fn layout(&mut self, width: u16, height: u16) {
        self.layout_node(self.root, width, height);
    }

    /// Mark all components as dirty (need re-render)
    pub fn mark_all_dirty(&mut self) {
        self.dirty.clear();
        for i in 0..self.components.len() {
            self.dirty.push(i);
        }
    }

    fn layout_node(&mut self, id: ComponentId, available_width: u16, available_height: u16) {
        // Calculate this component's size based on preferences
        let formatting = self.formatting.get(id).copied().unwrap_or_default();

        let component_width = self.calculate_size(formatting.preferred_width, available_width);
        let component_height = self.calculate_size(formatting.preferred_height, available_height);

        // Set rect for root node only (since parent layout functions handle non-root nodes)
        if id == self.root {
            let rect = Rect {
                x: 0,
                y: 0,
                width: component_width,
                height: component_height,
            };
            if let Some(rect_slot) = self.rects.get_mut(id) {
                *rect_slot = rect;
            }
        }

        // Layout children
        if let Some(child_ids) = self.children(id) {
            // Get layout mode from formatting
            let layout_mode = self
                .formatting
                .get(id)
                .copied()
                .unwrap_or_default()
                .layout_mode;
            self.layout_children(child_ids, component_width, component_height, layout_mode);
        }
    }

    fn layout_children(
        &mut self,
        child_ids: Vec<ComponentId>,
        available_width: u16,
        available_height: u16,
        layout_mode: LayoutMode,
    ) {
        if child_ids.is_empty() {
            return;
        }

        match layout_mode {
            LayoutMode::HorizontalSplit => {
                self.layout_children_horizontal(child_ids, available_width, available_height)
            }
            LayoutMode::VerticalSplit => {
                self.layout_children_vertical(child_ids, available_width, available_height)
            }
        }
    }

    fn layout_children_horizontal(
        &mut self,
        child_ids: Vec<ComponentId>,
        available_width: u16,
        available_height: u16,
    ) {
        // First pass: calculate space used by non-Fill components and count Fill components
        let mut used_width = 0u16;
        let mut fill_count = 0;
        let mut widths: Vec<u16> = Vec::new();

        for child_id in child_ids.iter() {
            let formatting = self.formatting.get(*child_id).copied().unwrap_or_default();
            match formatting.preferred_width {
                Measurement::Fill => {
                    fill_count += 1;
                    widths.push(0); // Placeholder, will be calculated
                }
                Measurement::Content => {
                    // Measure the component's rendered content
                    let (measured_width, _) =
                        self.measure_component(*child_id, available_width, available_height);
                    used_width = used_width.saturating_add(measured_width);
                    widths.push(measured_width);
                }
                _ => {
                    let width = self.calculate_size(formatting.preferred_width, available_width);
                    used_width = used_width.saturating_add(width);
                    widths.push(width);
                }
            }
        }

        // Calculate width for Fill components
        let remaining_width = available_width.saturating_sub(used_width);
        let fill_width = if fill_count > 0 {
            remaining_width / fill_count as u16
        } else {
            0
        };

        // Update widths for Fill components
        for (i, width) in widths.iter_mut().enumerate() {
            if *width == 0 && i < child_ids.len() {
                let formatting = self
                    .formatting
                    .get(child_ids[i])
                    .copied()
                    .unwrap_or_default();
                if matches!(formatting.preferred_width, Measurement::Fill) {
                    *width = fill_width;
                }
            }
        }

        // Layout children with calculated widths
        let mut current_x = 0u16;
        let mut current_y = 0u16;
        let mut max_height_in_row = 1u16;

        for (i, child_id) in child_ids.iter().enumerate() {
            let formatting = self.formatting.get(*child_id).copied().unwrap_or_default();
            let component_width = widths[i];
            let component_height = if matches!(formatting.preferred_height, Measurement::Content) {
                // Measure the component's rendered content with the actual component width
                // This is critical for components with overflow_x: Wrap to calculate correct height
                let (_, measured_height) =
                    self.measure_component(*child_id, component_width, available_height);
                measured_height
            } else {
                self.calculate_size(formatting.preferred_height, available_height)
            };

            // Check if component fits on current line
            let fits_on_line = current_x + component_width <= available_width;

            // Determine behavior based on overflow and fit
            if !fits_on_line && formatting.overflow_x == Overflow::Wrap {
                // Move to next line
                current_x = 0;
                current_y += max_height_in_row;
                max_height_in_row = 1;
            }

            // Check if we've exceeded available height
            if current_y >= available_height {
                break;
            }

            let rect = Rect {
                x: current_x,
                y: current_y,
                width: component_width,
                height: component_height,
            };

            // Store rect in tree's rects Vec
            if let Some(rect_slot) = self.rects.get_mut(*child_id) {
                *rect_slot = rect;
            }

            // Update position for next child
            current_x += component_width;
            max_height_in_row = max_height_in_row.max(component_height);

            // Recursively layout this child
            self.layout_node(*child_id, component_width, component_height);
        }
    }

    fn layout_children_vertical(
        &mut self,
        child_ids: Vec<ComponentId>,
        available_width: u16,
        available_height: u16,
    ) {
        // First pass: calculate space used by non-Fill components and count Fill components
        let mut used_height = 0u16;
        let mut fill_count = 0;
        let mut heights: Vec<u16> = Vec::new();

        for child_id in child_ids.iter() {
            let formatting = self.formatting.get(*child_id).copied().unwrap_or_default();
            match formatting.preferred_height {
                Measurement::Fill => {
                    fill_count += 1;
                    heights.push(0); // Placeholder, will be calculated
                }
                Measurement::Content => {
                    // Measure the component's rendered content
                    let (_, measured_height) =
                        self.measure_component(*child_id, available_width, available_height);
                    used_height = used_height.saturating_add(measured_height);
                    heights.push(measured_height);
                }
                _ => {
                    let height = self.calculate_size(formatting.preferred_height, available_height);
                    used_height = used_height.saturating_add(height);
                    heights.push(height);
                }
            }
        }

        // Calculate height for Fill components
        let remaining_height = available_height.saturating_sub(used_height);
        let fill_height = if fill_count > 0 {
            remaining_height / fill_count as u16
        } else {
            0
        };

        // Update heights for Fill components
        for (i, height) in heights.iter_mut().enumerate() {
            if *height == 0 && i < child_ids.len() {
                let formatting = self
                    .formatting
                    .get(child_ids[i])
                    .copied()
                    .unwrap_or_default();
                if matches!(formatting.preferred_height, Measurement::Fill) {
                    *height = fill_height;
                }
            }
        }

        // Layout children with calculated heights
        let mut current_x = 0u16;
        let mut current_y = 0u16;
        let mut max_width_in_column = 1u16;

        for (i, child_id) in child_ids.iter().enumerate() {
            let formatting = self.formatting.get(*child_id).copied().unwrap_or_default();
            let component_width = if matches!(formatting.preferred_width, Measurement::Content) {
                // Measure the component's rendered content
                let (measured_width, _) =
                    self.measure_component(*child_id, available_width, available_height);
                measured_width
            } else {
                self.calculate_size(formatting.preferred_width, available_width)
            };
            let component_height = heights[i];

            // Check if component fits on current column
            let fits_in_column = current_y + component_height <= available_height;

            // Determine behavior based on overflow and fit
            if !fits_in_column && formatting.overflow_y == Overflow::Wrap {
                // Move to next column
                current_y = 0;
                current_x += max_width_in_column;
                max_width_in_column = 1;
            }

            // Check if we've exceeded available width
            if current_x >= available_width {
                break;
            }

            let rect = Rect {
                x: current_x,
                y: current_y,
                width: component_width,
                height: component_height,
            };

            // Store rect in tree's rects Vec
            if let Some(rect_slot) = self.rects.get_mut(*child_id) {
                *rect_slot = rect;
            }

            // Update position for next child
            current_y += component_height;
            max_width_in_column = max_width_in_column.max(component_width);

            // Recursively layout this child
            self.layout_node(*child_id, component_width, component_height);
        }
    }

    /// Render the entire tree to stdout
    pub fn render(&mut self, stdout: &mut Stdout) -> Result<()> {
        // Hide cursor during rendering
        stdout.execute(Hide)?;

        // Render all nodes
        self.render_node(self.root, stdout)?;

        // Show cursor if we found one
        if let Some((x, y)) = self.cursor_pos {
            stdout.execute(self.cursor_style[self.focus].to_command())?;
            stdout.execute(MoveTo(x, y))?;
            stdout.execute(Show)?;
        }

        self.clear_dirty();

        Ok(())
    }

    fn render_node(&mut self, id: ComponentId, stdout: &mut Stdout) -> Result<()> {
        let rect = self.rects.get(id).copied().unwrap_or_default();
        let formatting = self.formatting.get(id).copied().unwrap_or_default();

        // Cursor starts at (0, 0) by default
        if !self.cursor_initialized.get(id).copied().unwrap_or(false) {
            if let Some(init_slot) = self.cursor_initialized.get_mut(id) {
                *init_slot = true;
            }
        }

        // Get cursor position from tree (absolute values)
        let cursor_tree_col = self.cursor_col.get(id).copied().unwrap_or(0);
        let cursor_tree_row = self.cursor_row.get(id).copied().unwrap_or(0);

        // Get current scroll
        let scroll_x = self.scroll_x.get(id).copied().unwrap_or(0);
        let scroll_y = self.scroll_y.get(id).copied().unwrap_or(0);

        // Handle cursor position and scrolling for focused component (before rendering)
        let mut final_scroll_y = scroll_y;

        // Now get the component after all mutable operations are done
        if let Some(component) = self.components.get(id) {
            // Create a virtual buffer for this component
            let mut buffer = TerminalBuffer::new(rect.width, rect.height);

            // Set focus if this component is focused
            if id == self.focus {
                buffer.set_focus(true);
            }

            // Set scroll offset in buffer
            buffer.set_scroll(scroll_x, final_scroll_y);

            // Convert absolute cursor position to relative position within visible area
            let mut relative_row = (cursor_tree_row as usize).saturating_sub(final_scroll_y);
            let mut relative_col = (cursor_tree_col as usize).saturating_sub(scroll_x) as u16;

            // Set cursor position in buffer as component-relative
            buffer.set_cursor_position(relative_col, relative_row as u16);

            // Check dirty AFTER we've processed scroll and potentially marked as dirty
            let is_dirty_now = self.is_dirty(id);

            // Render component into the virtual buffer
            if is_dirty_now {
                component.render(&mut buffer)?;

                // Only composite if the buffer has content
                if !buffer.commands().is_empty() {
                    stdout.execute(ResetColor)?;

                    let max_lines = rect.height;

                    match formatting.overflow_x {
                        Overflow::Wrap => {
                            self.composite_buffer_wrap(
                                stdout, &buffer, rect.x, rect.y, 0, max_lines,
                            )?;
                        }
                        Overflow::Hide | Overflow::Scroll => {
                            self.composite_buffer_hide(
                                stdout, &buffer, rect.x, rect.y, 0, max_lines,
                            )?;
                        }
                    }
                }
            }

            // For wrapped components, calculate which visual wrapped line the cursor should appear on
            if formatting.overflow_x == Overflow::Wrap && relative_col >= rect.width {
                // Recalculate cursor position accounting for wrapping
                let mut remaining_col = relative_col as usize;
                let mut visual_row = relative_row;

                // Walk through wrapped lines to find which visual row the cursor is on
                loop {
                    let row_width = rect.width as usize;
                    if remaining_col < row_width {
                        // Cursor falls within this visual row
                        relative_col = remaining_col as u16;
                        relative_row = visual_row;
                        break;
                    }

                    remaining_col -= row_width;
                    visual_row += 1;

                    // Safety check: if we've gone beyond the visible area, clamp to last position
                    if visual_row >= rect.height as usize {
                        relative_row = (rect.height as usize).saturating_sub(1);
                        relative_col = (rect.width as u16).saturating_sub(1);
                        break;
                    }
                }
            }

            // Store final screen cursor position for terminal output
            if id == self.focus
                && relative_row < rect.height as usize
                && (relative_col as u16) < rect.width
            {
                let cursor_col = rect.x + relative_col as u16;
                let cursor_row = rect.y + relative_row as u16;
                self.cursor_pos = Some((cursor_col, cursor_row));
            }
        }

        // Render children
        if let Some(child_ids_vec) = self.children(id) {
            // Get this component's scroll offset and bounds
            let parent_scroll_y = self.scroll_y.get(id).copied().unwrap_or(0);
            let parent_rect = self.rects.get(id).copied().unwrap_or_default();

            // Mark visible children as dirty
            if formatting.layout_mode == LayoutMode::VerticalSplit {
                // For vertical layouts, mark children based on scroll position
                let mut current_y = parent_rect.y;
                for (child_index, child_id) in child_ids_vec.iter().enumerate() {
                    // Skip children before scroll position
                    if child_index < parent_scroll_y {
                        continue;
                    }

                    // Get child height
                    if let Some(child_rect) = self.rects.get(*child_id) {
                        let child_height = child_rect.height as u16;

                        // Stop if child would render below parent
                        if current_y >= parent_rect.y + parent_rect.height {
                            break;
                        }

                        // Mark as dirty if it might be visible
                        self.mark_dirty(*child_id);

                        current_y += child_height;
                    }
                }
            } else {
                // For horizontal layouts, mark all children as dirty
                for child_id in child_ids_vec.iter() {
                    self.mark_dirty(*child_id);
                }
            }

            // Update focus if focused child has scrolled out of view
            if formatting.layout_mode == LayoutMode::VerticalSplit {
                let visible_end = parent_scroll_y + (parent_rect.height as usize);

                // Find which direct child of this container is an ancestor of the current focus
                let mut focused_component = self.focus;
                let mut focused_child_of_container = None;

                // Trace up from focus until we find a child of this container or reach the root
                loop {
                    if let Some(parent_id) = self.parent.get(focused_component).and_then(|p| *p) {
                        if parent_id == id {
                            // This is a direct child of our container
                            focused_child_of_container = Some(focused_component);
                            break;
                        }
                        focused_component = parent_id;
                    } else {
                        // We've reached the root without finding a child of this container
                        break;
                    }
                }

                // If we found a focused child of this container, check if it's in the visible range
                if let Some(focused_child) = focused_child_of_container {
                    // Find the index of the focused child
                    let focused_child_index = child_ids_vec.iter().position(|&c| c == focused_child);

                    if let Some(child_idx) = focused_child_index {
                        if child_idx < parent_scroll_y {
                            // Focused child is above visible range, clamp to first visible
                            if let Some(closest_child) = child_ids_vec.get(parent_scroll_y) {
                                let deepest = self.find_last_focusable_descendant(*closest_child);
                                // Get cursor position from the leaf component that we're moving focus away from
                                if let (Some(old_cursor_col), Some(old_cursor_row)) =
                                    (self.cursor_col.get(self.focus).copied(), self.cursor_row.get(self.focus).copied()) {
                                    // Set the same cursor position in the new focused component
                                    if let Some(cursor_col_slot) = self.cursor_col.get_mut(deepest) {
                                        *cursor_col_slot = old_cursor_col;
                                    }
                                    if let Some(cursor_row_slot) = self.cursor_row.get_mut(deepest) {
                                        *cursor_row_slot = old_cursor_row;
                                    }
                                }
                                self.focus = deepest;
                                self.update_focus_path();
                            }
                        } else if child_idx >= visible_end {
                            // Focused child is below visible range, clamp to last visible
                            let last_visible_idx = (visible_end - 1).min(child_ids_vec.len() - 1);
                            if let Some(closest_child) = child_ids_vec.get(last_visible_idx) {
                                let deepest = self.find_last_focusable_descendant(*closest_child);
                                // Get cursor position from the leaf component that we're moving focus away from
                                if let (Some(old_cursor_col), Some(old_cursor_row)) =
                                    (self.cursor_col.get(self.focus).copied(), self.cursor_row.get(self.focus).copied()) {
                                    // Set the same cursor position in the new focused component
                                    if let Some(cursor_col_slot) = self.cursor_col.get_mut(deepest) {
                                        *cursor_col_slot = old_cursor_col;
                                    }
                                    if let Some(cursor_row_slot) = self.cursor_row.get_mut(deepest) {
                                        *cursor_row_slot = old_cursor_row;
                                    }
                                }
                                self.focus = deepest;
                                self.update_focus_path();
                            }
                        }
                    }
                }
            }

            // Limit scrolling: prevent scrolling past the last child
            if formatting.overflow_y == Overflow::Scroll && !child_ids_vec.is_empty() {
                // Maximum scroll index is the number of children
                let max_scroll = child_ids_vec.len();

                // Clamp scroll to valid range
                if parent_scroll_y >= max_scroll {
                    if let Some(scroll) = self.scroll_y.get_mut(id) {
                        *scroll = max_scroll.saturating_sub(1);
                    }
                }
            }

            // Track the lowest point rendered by children
            let mut lowest_rendered_y: Option<u16> = None;
            let mut current_screen_y = parent_rect.y;

            for (child_index, child_id) in child_ids_vec.iter().enumerate() {
                // For vertical layouts, skip children before the scroll position
                if formatting.layout_mode == LayoutMode::VerticalSplit
                    && child_index < final_scroll_y
                {
                    continue;
                }

                // Get child height and original Y position
                let (original_y, child_height) = {
                    if let Some(child_rect) = self.rects.get(*child_id) {
                        (child_rect.y, child_rect.height as usize)
                    } else {
                        continue;
                    }
                };

                // Stop if this child would render completely below the parent (vertical layouts only)
                if formatting.layout_mode == LayoutMode::VerticalSplit
                    && current_screen_y >= parent_rect.y + parent_rect.height
                {
                    break;
                }

                // Stop if child doesn't fit completely in visible range (atomic rendering, vertical only)
                if formatting.layout_mode == LayoutMode::VerticalSplit
                    && current_screen_y + child_height as u16 > parent_rect.y + parent_rect.height
                {
                    break;
                }

                // For vertical layouts, adjust screen position based on scroll
                if formatting.layout_mode == LayoutMode::VerticalSplit {
                    let child_screen_bottom = current_screen_y + child_height as u16;

                    // Update lowest rendered point
                    lowest_rendered_y = Some(match lowest_rendered_y {
                        None => child_screen_bottom,
                        Some(current_lowest) => current_lowest.max(child_screen_bottom),
                    });

                    // Temporarily adjust child's y position for rendering
                    {
                        if let Some(child_rect_mut) = self.rects.get_mut(*child_id) {
                            child_rect_mut.y = current_screen_y;
                        }
                    } // Drop the mutable borrow

                    // Render child
                    let _ = self.render_node(*child_id, stdout);

                    // Restore original y position for next frame
                    {
                        if let Some(child_rect_mut) = self.rects.get_mut(*child_id) {
                            child_rect_mut.y = original_y;
                        }
                    }

                    // Advance screen position for next child
                    current_screen_y += child_height as u16;
                } else {
                    // For horizontal layouts, convert relative rect positions to absolute screen positions
                    let (original_x, child_width) = {
                        if let Some(child_rect) = self.rects.get(*child_id) {
                            (child_rect.x, child_rect.width)
                        } else {
                            continue;
                        }
                    };

                    let child_screen_y = parent_rect.y + original_y as u16;
                    let child_screen_x = parent_rect.x + original_x;
                    let child_screen_bottom = child_screen_y + child_height as u16;
                    lowest_rendered_y = Some(match lowest_rendered_y {
                        None => child_screen_bottom,
                        Some(current_lowest) => current_lowest.max(child_screen_bottom),
                    });

                    // Temporarily adjust child's position to absolute screen coordinates
                    {
                        if let Some(child_rect_mut) = self.rects.get_mut(*child_id) {
                            child_rect_mut.x = child_screen_x;
                            child_rect_mut.y = child_screen_y;
                        }
                    }

                    // Render child
                    let _ = self.render_node(*child_id, stdout);

                    // Restore original position for next frame
                    {
                        if let Some(child_rect_mut) = self.rects.get_mut(*child_id) {
                            child_rect_mut.x = original_x;
                            child_rect_mut.y = original_y as u16;
                        }
                    }
                }
            }

            // Fill remaining space below children with whitespace
            if let Some(lowest_y) = lowest_rendered_y {
                let parent_bottom = parent_rect.y + parent_rect.height;
                if lowest_y < parent_bottom {
                    for y in lowest_y..parent_bottom {
                        for x in parent_rect.x..(parent_rect.x + parent_rect.width) {
                            stdout.execute(MoveTo(x, y))?;
                            stdout.execute(Print(' '))?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn composite_buffer_hide(
        &self,
        stdout: &mut Stdout,
        buffer: &TerminalBuffer,
        start_x: u16,
        start_y: u16,
        skip_lines: u16,
        max_lines: u16,
    ) -> Result<()> {
        let mut x = 0u16;
        let mut screen_y = 0u16; // Track position on screen
        let mut skipped_newlines = 0u16; // Track logical lines (Newline commands only)

        for cmd in buffer.commands() {
            // Skip logical lines until we reach skip_lines
            // Count both Newline commands and wrapped rows as line boundaries
            if skipped_newlines < skip_lines {
                match cmd {
                    TerminalCommand::Newline => {
                        skipped_newlines += 1;
                        x = 0;
                    }
                    TerminalCommand::Print(_) => {
                        x += 1;
                        if x >= buffer.width() {
                            skipped_newlines += 1;
                            x = 0;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // Exit if we've rendered max_lines on screen
            if screen_y >= max_lines {
                break;
            }

            match cmd {
                TerminalCommand::Print(ch) => {
                    if x >= buffer.width() {
                        // Skip this character (hide overflow)
                        continue;
                    }

                    // Move to position and print
                    stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
                    stdout.execute(Print(ch))?;
                    x += 1;
                }
                TerminalCommand::Newline => {
                    // Reset colors before newline
                    stdout.execute(ResetColor)?;
                    // Pad the rest of the line with spaces
                    while x < buffer.width() {
                        stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
                        stdout.execute(Print(' '))?;
                        x += 1;
                    }
                    x = 0;
                    screen_y += 1;
                }
                TerminalCommand::SetForeground(color) => {
                    stdout.execute(SetForegroundColor(*color))?;
                }
                TerminalCommand::SetBackground(color) => {
                    stdout.execute(SetBackgroundColor(*color))?;
                }
                TerminalCommand::Clear => {
                    // Skip for now
                }
            }
        }

        // Always pad the current line if we have content, even if we hit max_lines
        if x > 0 || screen_y > 0 {
            stdout.execute(ResetColor)?;
            while x < buffer.width() {
                stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
                stdout.execute(Print(' '))?;
                x += 1;
            }
        }

        // Clear any remaining screen rows beyond what was rendered
        let mut clear_y = screen_y + 1;
        while clear_y < max_lines {
            for x_pos in 0..buffer.width() {
                stdout.execute(MoveTo(start_x + x_pos, start_y + clear_y))?;
                stdout.execute(Print(' '))?;
            }
            clear_y += 1;
        }

        Ok(())
    }

    fn composite_buffer_wrap(
        &self,
        stdout: &mut Stdout,
        buffer: &TerminalBuffer,
        start_x: u16,
        start_y: u16,
        skip_lines: u16,
        max_lines: u16,
    ) -> Result<()> {
        let mut x = 0u16;
        let mut screen_y = 0u16; // Track position on screen
        let mut skipped_newlines = 0u16; // Track logical lines (Newline commands only)

        for cmd in buffer.commands() {
            // Skip logical lines until we reach skip_lines
            // Count both Newline commands and wrapped rows as line boundaries
            if skipped_newlines < skip_lines {
                match cmd {
                    TerminalCommand::Newline => {
                        skipped_newlines += 1;
                        x = 0;
                    }
                    TerminalCommand::Print(_ch) => {
                        x += 1;
                        if x >= buffer.width() {
                            skipped_newlines += 1;
                            x = 0;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // Exit if we've rendered max_lines on screen
            if screen_y >= max_lines {
                break;
            }

            match cmd {
                TerminalCommand::Print(ch) => {
                    // Wrap to next line if we overflow width
                    if x >= buffer.width() {
                        // Pad the line we're leaving
                        while x < buffer.width() {
                            stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
                            stdout.execute(Print(' '))?;
                            x += 1;
                        }
                        x = 0;
                        screen_y += 1;
                    }

                    // Check bounds after wrap but before rendering
                    if screen_y >= max_lines {
                        break;
                    }

                    // Move to position and print
                    stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
                    stdout.execute(Print(ch))?;
                    x += 1;
                }
                TerminalCommand::Newline => {
                    // Pad the rest of the line with spaces
                    while x < buffer.width() {
                        stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
                        stdout.execute(Print(' '))?;
                        x += 1;
                    }
                    stdout.execute(ResetColor)?;
                    x = 0;
                    screen_y += 1;
                }
                TerminalCommand::SetForeground(color) => {
                    stdout.execute(SetForegroundColor(*color))?;
                }
                TerminalCommand::SetBackground(color) => {
                    stdout.execute(SetBackgroundColor(*color))?;
                }
                TerminalCommand::Clear => {
                    // Skip for now
                }
            }
        }

        // Always pad the current row and clear remaining rows to avoid stale content
        while x < buffer.width() {
            stdout.execute(MoveTo(start_x + x, start_y + screen_y))?;
            stdout.execute(Print(' '))?;
            x += 1;
        }
        stdout.execute(ResetColor)?;

        // Clear any remaining screen rows beyond what was rendered
        let mut clear_y = screen_y + 1;
        while clear_y < max_lines {
            for x_pos in 0..buffer.width() {
                stdout.execute(MoveTo(start_x + x_pos, start_y + clear_y))?;
                stdout.execute(Print(' '))?;
            }
            clear_y += 1;
        }

        Ok(())
    }

    /// Find the scrollable container at the given screen position
    fn find_scrollable_container_at(&self, col: u16, row: u16) -> Option<ComponentId> {
        // Traverse all components to find which one contains this position
        let mut result = None;
        for (id, rect) in self.rects.iter().enumerate() {
            if rect.contains(col, row) {
                result = Some(id);
            }
        }

        // If we found a component, find its scrollable container ancestor
        if let Some(mut component_id) = result {
            loop {
                let formatting = self
                    .formatting
                    .get(component_id)
                    .copied()
                    .unwrap_or_default();

                // Check if this component is a scrollable container with children
                if formatting.overflow_y == Overflow::Scroll {
                    if let Some(children) = self.children(component_id) {
                        if !children.is_empty() {
                            return Some(component_id);
                        }
                    }
                }

                // Move to parent if it exists
                if let Some(parent_id) = self.parent.get(component_id).and_then(|p| *p) {
                    component_id = parent_id;
                } else {
                    break;
                }
            }
        }

        None
    }

    /// Check if child is a descendant of parent
    fn is_descendant_of(&self, potential_parent: ComponentId, child: ComponentId) -> bool {
        let mut current = child;
        while let Some(parent) = self.parent.get(current).and_then(|p| *p) {
            if parent == potential_parent {
                return true;
            }
            current = parent;
        }
        false
    }

    /// Update the focus path to match current focus
    fn update_focus_path(&mut self) {
        self.focus_path.clear();
        let mut current = self.focus;
        while let Some(parent) = self.parent.get(current).and_then(|p| *p) {
            self.focus_path.insert(0, current);
            current = parent;
        }
        self.focus_path.insert(0, current); // Add root
    }

    /// Find the last focusable descendant of a component
    fn find_last_focusable_descendant(&self, id: ComponentId) -> ComponentId {
        // Check if this component is focusable
        let formatting = self.formatting.get(id).copied().unwrap_or_default();
        if !formatting.focusable {
            return id; // Return the component even if not focusable, better than nothing
        }

        // Try to find focusable descendants
        if let Some(children) = self.children(id) {
            if !children.is_empty() {
                // Recursively check the last child
                if let Some(&last_child) = children.last() {
                    return self.find_last_focusable_descendant(last_child);
                }
            }
        }

        // No focusable descendants, return this component
        id
    }

    /// Scroll a component by the given amount in the Y direction
    fn scroll_by(&mut self, id: ComponentId, amount: isize) {
        let current_scroll = self.scroll_y.get(id).copied().unwrap_or(0);

        // Calculate new scroll with bounds checking
        let new_scroll = if amount > 0 {
            let max_scroll = if let Some(child_ids) = self.children(id) {
                child_ids.len().saturating_sub(1)
            } else {
                0
            };
            (current_scroll as isize + amount)
                .min(max_scroll as isize)
                .max(0) as usize
        } else {
            (current_scroll as isize + amount).max(0) as usize
        };

        if new_scroll != current_scroll {
            self.scroll_y.insert(id, new_scroll);
            self.mark_dirty(id);
            if let Some(child_ids) = self.children(id) {
                for child_id in child_ids {
                    self.mark_dirty(child_id);
                }
            }


            // Clamp cursor to visible range after scrolling
            let rect = self.rects.get(id).copied().unwrap_or_default();
            let visible_height = rect.height as usize;
            let cursor_tree_row = self.cursor_row.get(id).copied().unwrap_or(0);

            // If cursor is above visible range, move it to the top
            if (cursor_tree_row as usize) < new_scroll {
                if let Some(cursor) = self.cursor_row.get_mut(id) {
                    *cursor = new_scroll as u16;
                }
            } else if (cursor_tree_row as usize) >= new_scroll + visible_height {
                // If cursor is below visible range, move it to the bottom
                let new_cursor_row = (new_scroll + visible_height.saturating_sub(1)) as u16;
                if let Some(cursor) = self.cursor_row.get_mut(id) {
                    *cursor = new_cursor_row;
                }
            }
        }
    }

    /// Handle an event for the entire tree
    pub fn update(&mut self, event: ReovimEvent) -> Result<()> {
        // Handle scroll events at tree level
        if let ReovimEvent::Mouse(mouse_event) = &event {
            match mouse_event.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let rect = self.rect(self.focus).unwrap_or_default();
                    if rect.contains(mouse_event.column, mouse_event.row) {
                        self.update_node(self.root, &event)?;
                    }
                    return Ok(());
                }
                MouseEventKind::ScrollUp => {
                    if let Some(container_id) =
                        self.find_scrollable_container_at(mouse_event.column, mouse_event.row)
                    {
                        self.scroll_by(container_id, -1);
                    }
                    return Ok(());
                }
                MouseEventKind::ScrollDown => {
                    if let Some(container_id) =
                        self.find_scrollable_container_at(mouse_event.column, mouse_event.row)
                    {
                        self.scroll_by(container_id, 1);
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        // For other events, pass to root
        self.update_node(self.root, &event)?;

        // After handling events, initialize any pending components
        self.initialize_pending_components()?;
        Ok(())
    }

    /// Initialize children for all pending components
    pub fn initialize_pending_components(&mut self) -> Result<()> {
        // Keep processing until all pending components are initialized
        // This handles cases where components add children during their own initialization
        while !self.pending_initialization.is_empty() {
            let pending = std::mem::take(&mut self.pending_initialization);

            for comp_id in pending {
                // Use unsafe pointer to avoid borrow checker issues
                // This is safe because initialize_children only needs to add children,
                // which is a safe operation on the tree
                unsafe {
                    let tree_ptr = self as *mut ComponentTree<'a>;
                    let mut commands = ComponentCommands::new(&mut *tree_ptr, comp_id);
                    if let Some(component) = (&mut *tree_ptr).components.get_mut(comp_id) {
                        component.initialize_children(&mut commands)?;
                    }
                }
            }
        }

        // After all components are initialized, ensure focus descends to first focusable child
        self.ensure_focus_descends_to_leaf();

        Ok(())
    }

    /// Ensure the focused component descends to its first focusable child (recursively)
    /// This is called after initialization to ensure components with children automatically
    /// descend to the deepest focusable leaf
    fn ensure_focus_descends_to_leaf(&mut self) {
        let focused_id = self.focus;
        let focus_id = self.find_deepest_focusable_descendant(focused_id);

        if focus_id != focused_id {
            self.focus = focus_id;
            self.focus_path = self.build_focus_path(focus_id);
            self.mark_dirty(focus_id);
        }
    }

    /// Find the deepest focusable descendant of a component (for use during initialization)
    /// This is similar to the logic in ComponentCommands but works within the tree
    fn find_deepest_focusable_descendant(&self, component_id: ComponentId) -> ComponentId {
        let formatting = self
            .formatting
            .get(component_id)
            .copied()
            .unwrap_or_default();

        // If this component is focusable, check if it has focusable children to descend into
        if formatting.focusable {
            if let Some(children) = self.children(component_id) {
                // Try to find the first focusable child
                for &child_id in &children {
                    let child_formatting =
                        self.formatting.get(child_id).copied().unwrap_or_default();
                    if self.is_focusable_or_has_focusable_child(child_id, child_formatting) {
                        // Recursively find the deepest focusable descendant of this child
                        return self.find_deepest_focusable_descendant(child_id);
                    }
                }
            }
            // No focusable children, return this component
            return component_id;
        }

        // If not focusable, try to find a focusable descendant
        if let Some(children) = self.children(component_id) {
            for &child_id in &children {
                let child_formatting = self.formatting.get(child_id).copied().unwrap_or_default();
                if self.is_focusable_or_has_focusable_child(child_id, child_formatting) {
                    return self.find_deepest_focusable_descendant(child_id);
                }
            }
        }

        component_id
    }

    /// Helper to check if a component is focusable or has focusable children
    fn is_focusable_or_has_focusable_child(
        &self,
        component_id: ComponentId,
        formatting: Formatting,
    ) -> bool {
        if formatting.focusable {
            return true;
        }

        // Check if this component has any focusable children
        if let Some(children) = self.children(component_id) {
            for &child_id in &children {
                let child_formatting = self.formatting.get(child_id).copied().unwrap_or_default();
                if self.is_focusable_or_has_focusable_child(child_id, child_formatting) {
                    return true;
                }
            }
        }

        false
    }

    fn mark_dirty(&mut self, id: ComponentId) {
        if !self.dirty.contains(&id) {
            self.dirty.push(id);
        }
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.clear()
    }

    fn is_dirty(&self, id: ComponentId) -> bool {
        self.dirty.contains(&id)
    }

    fn update_node(&mut self, id: ComponentId, event: &ReovimEvent) -> Result<()> {
        // SAFETY: We need a mutable reference to both the component and the tree.
        // We use an unsafe pointer cast to work around the borrow checker.
        // This is safe because the component's update method only borrows the tree
        // to query/modify other components, and we don't use the component reference after update.
        unsafe {
            let tree_ptr = self as *mut ComponentTree<'a>;
            if let Some(component) = self.components.get_mut(id) {
                let mut commands = ComponentCommands::new(&mut *tree_ptr, id);
                let dirty = component.update(event.clone(), &mut commands)?;
                if dirty {
                    self.mark_dirty(id);
                }
            }
        }

        if let Some(child_ids) = self.children(id) {
            for child_id in child_ids {
                self.update_node(child_id, event)?;
            }
        }

        Ok(())
    }
}
