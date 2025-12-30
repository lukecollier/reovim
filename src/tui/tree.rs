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

pub type ComponentId = usize;

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
        // Get rect dimensions, formatting, and cursor bounds from the component
        let rect = self
            .tree
            .rects
            .get(self.self_id)
            .copied()
            .unwrap_or_default();
        let formatting = self
            .tree
            .formatting
            .get(self.self_id)
            .copied()
            .unwrap_or_default();

        // Get full content bounds including children
        let (content_width, content_height) = self.tree.get_content_bounds(self.self_id);

        let (min_col, max_col, min_row, max_row) = self
            .tree
            .components
            .get(self.self_id)
            .map(|comp| comp.cursor_bounds(content_width, content_height, &formatting))
            .unwrap_or((0, u16::MAX, 0, u16::MAX));

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
    /// Implements hierarchical navigation:
    /// - In VerticalSplit: Up/Down = move between children, Left/Right = wrap within child
    /// - In HorizontalSplit: Left/Right = move between children, Up/Down = wrap within child
    pub fn move_cursor(&mut self, col_delta: i32, row_delta: i32) {
        // If a deeper component is focused (this component is an ancestor),
        // work with the deepest focused component instead
        let current_id = if !self.tree.focus_path.is_empty() {
            let deepest_focused = *self.tree.focus_path.last().unwrap();
            if deepest_focused != self.self_id {
                // Use the deepest focused component for movement
                deepest_focused
            } else {
                self.self_id
            }
        } else {
            self.self_id
        };
        let formatting = self
            .tree
            .formatting
            .get(current_id)
            .copied()
            .unwrap_or_default();

        // For leaf components, measure actual content width; for containers, use full content bounds
        let (content_width, content_height) = if self.tree.children(current_id).map_or(true, |c| c.is_empty()) {
            // Leaf component - measure actual rendered content
            self.tree.measure_component(current_id, u16::MAX, u16::MAX)
        } else {
            // Container with children - use full content bounds
            self.tree.get_content_bounds(current_id)
        };

        // Get cursor bounds from the component
        let (min_col, max_col, min_row, max_row) = self
            .tree
            .components
            .get(current_id)
            .map(|comp| comp.cursor_bounds(content_width, content_height, &formatting))
            .unwrap_or((0, u16::MAX, 0, u16::MAX));

        // First, check if we can navigate within this component's children
        let current_formatting = self
            .tree
            .formatting
            .get(current_id)
            .copied()
            .unwrap_or_default();

        let can_navigate_current_children = matches!(
            (
                current_formatting.layout_mode,
                col_delta != 0,
                row_delta != 0
            ),
            (LayoutMode::VerticalSplit, false, true) | // Up/Down in VerticalSplit
            (LayoutMode::HorizontalSplit, true, false) // Left/Right in HorizontalSplit
        ) && self
            .tree
            .children(current_id)
            .map_or(false, |c| !c.is_empty());

        if can_navigate_current_children {
            // Navigate within current component's children
            self.navigate_children(current_id, col_delta, row_delta);
            return;
        }

        // Otherwise, check if we can navigate to siblings
        let parent_id = self.tree.parent(current_id).and_then(|p| p);

        if let Some(parent) = parent_id {
            let parent_layout = self
                .tree
                .formatting
                .get(parent)
                .copied()
                .unwrap_or_default()
                .layout_mode;

            let is_child_nav = matches!(
                (parent_layout, col_delta != 0, row_delta != 0),
                (LayoutMode::VerticalSplit, false, true) | // Up/Down in VerticalSplit
                (LayoutMode::HorizontalSplit, true, false) // Left/Right in HorizontalSplit
            );

            if is_child_nav {
                // Navigate between sibling children
                self.navigate_siblings(parent, col_delta, row_delta);
                return;
            }
        }

        // Navigate within wrapped content of current component
        self.navigate_within_component(
            current_id, col_delta, row_delta, 0, min_col, max_col, min_row, max_row,
        );
    }

    /// Navigate between children of the current component
    fn navigate_children(&mut self, parent_id: ComponentId, col_delta: i32, row_delta: i32) {
        if let Some(children) = self.tree.children(parent_id) {
            if children.is_empty() {
                return;
            }

            // Find current position in children by looking for the currently focused child
            // Use the deepest focused child in the focus path that's a child of this parent
            let mut current_pos: i32 = -1;
            for focus_id in &self.tree.focus_path {
                if let Some(pos) = children.iter().position(|&id| id == *focus_id) {
                    current_pos = pos as i32;
                    break; // Use the first (closest) one found
                }
            }

            // Determine next child based on movement direction
            let next_pos = if row_delta > 0 {
                ((current_pos + 1).max(0) as usize).min(children.len() - 1)
            } else if row_delta < 0 {
                (current_pos - 1).max(0) as usize
            } else if col_delta > 0 {
                ((current_pos + 1).max(0) as usize).min(children.len() - 1)
            } else if col_delta < 0 {
                (current_pos - 1).max(0) as usize
            } else {
                return;
            };

            // Only move if position changed
            if next_pos as i32 != current_pos {
                let next_id = children[next_pos];

                // Try to enter the next child and find the actual descendant to focus
                if self.try_enter_component(next_id) {
                    // Find the deepest focusable descendant to actually focus on
                    let focus_id = self.find_focusable_descendant(next_id);

                    // Update current focus and focus path
                    let old_id = self.self_id;
                    self.self_id = focus_id;
                    self.tree.focus = focus_id;
                    self.tree.focus_path = self.tree.build_focus_path(focus_id);
                    self.tree.mark_dirty(focus_id);
                    self.tree.mark_dirty(old_id);
                }
            }
        }
    }

    /// Navigate between sibling components
    fn navigate_siblings(&mut self, parent_id: ComponentId, col_delta: i32, row_delta: i32) {
        if let Some(siblings) = self.tree.children(parent_id) {
            if siblings.is_empty() {
                return;
            }

            // Find current position in siblings by looking for the currently focused sibling
            // Use the focused sibling from the focus path
            let mut current_pos = 0;
            for focus_id in &self.tree.focus_path {
                if let Some(pos) = siblings.iter().position(|&id| id == *focus_id) {
                    current_pos = pos;
                    break; // Use the first (closest) one found
                }
            }

            // Determine next sibling based on movement direction
            let next_pos = if row_delta > 0 {
                (current_pos + 1).min(siblings.len() - 1)
            } else if row_delta < 0 {
                current_pos.saturating_sub(1)
            } else if col_delta > 0 {
                (current_pos + 1).min(siblings.len() - 1)
            } else if col_delta < 0 {
                current_pos.saturating_sub(1)
            } else {
                return;
            };

            // Only move if position changed
            if next_pos != current_pos {
                let next_id = siblings[next_pos];

                // Try to enter the next component and find the actual descendant to focus
                if self.try_enter_component(next_id) {
                    // Find the deepest focusable descendant to actually focus on
                    let focus_id = self.find_focusable_descendant(next_id);

                    // Update current focus and focus path
                    self.self_id = focus_id;
                    self.tree.focus = focus_id;
                    self.tree.focus_path = self.tree.build_focus_path(focus_id);
                    self.tree.mark_dirty(focus_id);
                    self.tree.mark_dirty(siblings[current_pos]);
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

    /// Navigate within a component's wrapped content
    fn navigate_within_component(
        &mut self,
        component_id: ComponentId,
        col_delta: i32,
        row_delta: i32,
        _child_width: u16,
        min_col: u16,
        max_col: u16,
        min_row: u16,
        max_row: u16,
    ) {
        let current_col = self.tree.cursor_col.get(component_id).copied().unwrap_or(0);
        let current_row = self.tree.cursor_row.get(component_id).copied().unwrap_or(0);

        if col_delta != 0 {
            // Measure actual content width of the leaf component without width constraints
            // This gives us the true content width, not constrained by parent allocation
            let (content_width, _) = self.tree.measure_component(component_id, u16::MAX, u16::MAX);
            let clamped_max_col = (content_width as u16).saturating_sub(1).max(min_col);
            let effective_max_col = max_col.min(clamped_max_col);

            // Horizontal movement within wrapped content
            let new_col = if col_delta < 0 {
                current_col.saturating_sub((-col_delta) as u16)
            } else {
                current_col.saturating_add(col_delta as u16)
            };

            // Check if we're trying to move beyond content bounds
            let trying_to_move_right_beyond = col_delta > 0 && new_col > effective_max_col;
            let trying_to_move_left_beyond = col_delta < 0 && current_col == min_col;

            // Try to wrap to next/previous line if applicable
            let wrapped = if trying_to_move_right_beyond && current_row < max_row {
                // Can wrap to next line
                if let Some(col_slot) = self.tree.cursor_col.get_mut(component_id) {
                    *col_slot = min_col;
                }
                if let Some(row_slot) = self.tree.cursor_row.get_mut(component_id) {
                    *row_slot = current_row + 1;
                }
                true
            } else if trying_to_move_left_beyond && current_row > min_row {
                // Can wrap to previous line
                if let Some(col_slot) = self.tree.cursor_col.get_mut(component_id) {
                    *col_slot = effective_max_col;
                }
                if let Some(row_slot) = self.tree.cursor_row.get_mut(component_id) {
                    *row_slot = current_row - 1;
                }
                true
            } else {
                false
            };

            // If we didn't wrap, clamp the cursor to content bounds
            if !wrapped {
                let clamped_col = new_col.max(min_col).min(effective_max_col);
                if let Some(col_slot) = self.tree.cursor_col.get_mut(component_id) {
                    *col_slot = clamped_col;
                }
            }
        }

        if row_delta != 0 {
            // Vertical movement within wrapped content
            let new_row = if row_delta < 0 {
                current_row.saturating_sub((-row_delta) as u16)
            } else {
                current_row.saturating_add(row_delta as u16)
            };

            if let Some(row_slot) = self.tree.cursor_row.get_mut(component_id) {
                *row_slot = new_row.max(min_row).min(max_row);
            }
        }

        self.tree.mark_dirty(component_id);
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

    /// (min_col, max_col, min_row, max_row)
    pub fn cursor_bounds(
        &self,
        width: u16,
        height: u16,
        formatting: &Formatting,
    ) -> (u16, u16, u16, u16) {
        match self {
            ComponentNode::Frame(_) => (0, u16::MAX, 0, u16::MAX),
            ComponentNode::Status(component) => component.cursor_bounds(width, height, formatting),
            ComponentNode::Text(component) => component.cursor_bounds(width, height, formatting),
            ComponentNode::Debug(component) => component.cursor_bounds(width, height, formatting),
            ComponentNode::Component(component) => {
                component.cursor_bounds(width, height, formatting)
            }
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
    /// last_cursor_row[i] is the cursor row from the previous frame for component i
    last_cursor_row: Vec<u16>,
    /// cursor_initialized[i] tracks whether cursor has been set to minimum bounds for component i
    cursor_initialized: Vec<bool>,
    /// cursor_style[i] is the display style for the cursor of component i
    cursor_style: Vec<CursorStyle>,
    /// Temporary storage for skip_lines when rendering children with scroll offset
    skip_lines_override: std::collections::HashMap<ComponentId, u16>,
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
            last_cursor_row: vec![0],
            cursor_initialized: vec![false],
            cursor_style: vec![CursorStyle::default()],
            skip_lines_override: std::collections::HashMap::new(),
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
        self.last_cursor_row.push(0);
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
    fn measure_component(&self, id: ComponentId, max_width: u16, max_height: u16) -> (u16, u16) {
        let mut buffer = TerminalBuffer::new(max_width, max_height);

        if let Some(component) = self.components.get(id) {
            let _ = component.render(&mut buffer);
            buffer.measure_content()
        } else {
            (0, 0)
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
                // Measure the component's rendered content
                let (_, measured_height) =
                    self.measure_component(*child_id, available_width, available_height);
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

        // Clear previous cursor position
        self.cursor_pos = None;

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

        // Initialize cursor to minimum bounds on first render
        if !self.cursor_initialized.get(id).copied().unwrap_or(false) {
            if let Some(component) = self.components.get(id) {
                // Get full content bounds including children
                let (content_width, content_height) = self.get_content_bounds(id);
                let (min_col, _, min_row, _) =
                    component.cursor_bounds(content_width, content_height, &formatting);
                if let Some(col_slot) = self.cursor_col.get_mut(id) {
                    *col_slot = min_col;
                }
                if let Some(row_slot) = self.cursor_row.get_mut(id) {
                    *row_slot = min_row;
                }
                if let Some(init_slot) = self.cursor_initialized.get_mut(id) {
                    *init_slot = true;
                }
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
        if id == self.focus {
            let last_cursor_row = self.last_cursor_row.get(id).copied().unwrap_or(0);

            // Only auto-scroll if the cursor position changed (keyboard movement)
            let cursor_moved = cursor_tree_row != last_cursor_row;

            // Auto-scroll to keep cursor visible if component has Overflow::Scroll and cursor moved
            if formatting.overflow_y == Overflow::Scroll && cursor_moved {
                if let Some(component) = self.components.get(id) {
                    // Get full content bounds including children
                    let (content_width, content_height) = self.get_content_bounds(id);
                    let (_min_col, _max_col, min_row, max_row) =
                        component.cursor_bounds(content_width, content_height, &formatting);
                    let mut new_scroll_y = scroll_y;
                    let visible_height = rect.height as usize;

                    // Only scroll if cursor is within valid bounds
                    if cursor_tree_row >= min_row && cursor_tree_row <= max_row {
                        if cursor_tree_row < scroll_y as u16 {
                            // Cursor is above visible range, scroll up to show it
                            new_scroll_y = cursor_tree_row as usize;
                        } else if cursor_tree_row >= (scroll_y + visible_height) as u16 {
                            // Cursor is below visible range, scroll down to show it at bottom
                            new_scroll_y =
                                (cursor_tree_row as usize).saturating_sub(visible_height - 1);
                        }
                    }

                    // Update scroll and mark as dirty for re-render
                    if new_scroll_y != scroll_y {
                        final_scroll_y = new_scroll_y;
                        if let Some(scroll) = self.scroll_y.get_mut(id) {
                            *scroll = new_scroll_y;
                        }
                        self.mark_dirty(id);
                        // Also mark all children as dirty so they get re-rendered with new positions
                        if let Some(child_ids) = self.children(id) {
                            for child_id in child_ids {
                                self.mark_dirty(child_id);
                            }
                        }
                    }
                }
            }

            // Clamp cursor to visible range (for mouse scrolling)
            if formatting.overflow_y == Overflow::Scroll {
                if let Some(component) = self.components.get(id) {
                    // Get full content bounds including children
                    let (content_width, content_height) = self.get_content_bounds(id);
                    let (_min_col, _max_col, min_row, max_row) =
                        component.cursor_bounds(content_width, content_height, &formatting);
                    let visible_height = rect.height as usize;

                    // Only clamp if cursor is within valid bounds
                    if cursor_tree_row >= min_row && cursor_tree_row <= max_row {
                        // If cursor is above visible range, move it to the top
                        if cursor_tree_row < final_scroll_y as u16 {
                            if let Some(cursor) = self.cursor_row.get_mut(id) {
                                *cursor = final_scroll_y as u16;
                            }
                            self.mark_dirty(id);
                        } else if cursor_tree_row >= (final_scroll_y + visible_height) as u16 {
                            // If cursor is below visible range, move it to the bottom
                            let new_cursor_row =
                                (final_scroll_y + visible_height.saturating_sub(1)) as u16;
                            if let Some(cursor) = self.cursor_row.get_mut(id) {
                                *cursor = new_cursor_row;
                            }
                            self.mark_dirty(id);
                        }
                    }
                }
            }
        }

        // Scroll focused children into view
        if formatting.overflow_y == Overflow::Scroll {
            if let Some(child_ids) = self.children(id) {
                // Find if any child is in the focus path
                for focus_id in &self.focus_path {
                    if let Some(pos) = child_ids.iter().position(|&cid| cid == *focus_id) {
                        if let Some(child_rect) = self.rects.get(child_ids[pos]) {
                            let child_y = child_rect.y as usize;
                            let child_height = child_rect.height as usize;
                            let visible_height = rect.height as usize;

                            // Check if child is outside visible range
                            if child_y < final_scroll_y {
                                // Child is above visible range, scroll up to show it
                                final_scroll_y = child_y;
                                if let Some(scroll) = self.scroll_y.get_mut(id) {
                                    *scroll = final_scroll_y;
                                }
                                self.mark_dirty(id);
                                // Mark all children as dirty
                                for child_id in &child_ids {
                                    self.mark_dirty(*child_id);
                                }
                            } else if child_y + child_height > final_scroll_y + visible_height {
                                // Child is below visible range, scroll down to show it
                                final_scroll_y = (child_y + child_height).saturating_sub(visible_height);
                                if let Some(scroll) = self.scroll_y.get_mut(id) {
                                    *scroll = final_scroll_y;
                                }
                                self.mark_dirty(id);
                                // Mark all children as dirty
                                for child_id in &child_ids {
                                    self.mark_dirty(*child_id);
                                }
                            }
                        }
                        break; // Only scroll for the first focused child in this parent
                    }
                }
            }
        }

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
            let relative_row = (cursor_tree_row as usize).saturating_sub(final_scroll_y);
            let relative_col = (cursor_tree_col as usize).saturating_sub(scroll_x) as u16;

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

                    // Use skip_lines override if available (set by parent during child rendering)
                    let skip_lines = self.skip_lines_override.get(&id).copied().unwrap_or(0);
                    let max_lines = rect.height;

                    match formatting.overflow_x {
                        Overflow::Wrap => {
                            self.composite_buffer_wrap(
                                stdout, &buffer, rect.x, rect.y, skip_lines, max_lines,
                            )?;
                        }
                        Overflow::Hide | Overflow::Scroll => {
                            self.composite_buffer_hide(
                                stdout, &buffer, rect.x, rect.y, skip_lines, max_lines,
                            )?;
                        }
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

            // Update last_cursor_row for next frame's change detection
            if id == self.focus {
                if let Some(last_row) = self.last_cursor_row.get_mut(id) {
                    *last_row = cursor_tree_row;
                }
            }
        }

        // Render children
        if let Some(child_ids_vec) = self.children(id) {
            // Get this component's scroll offset and bounds
            let parent_scroll_y = self.scroll_y.get(id).copied().unwrap_or(0);
            let parent_rect = self.rects.get(id).copied().unwrap_or_default();

            // Mark only visible children as dirty so they render with updated positions
            // This applies to all components with children, not just scrollable ones
            let visible_start = parent_scroll_y;
            let visible_end = parent_scroll_y + parent_rect.height as usize;

            for child_id in child_ids_vec.iter() {
                if let Some(child_rect) = self.rects.get(*child_id) {
                    let original_y = child_rect.y as usize;
                    let child_height = child_rect.height as usize;
                    let child_end = original_y + child_height;

                    // Mark child as dirty only if it's at least partially visible
                    if child_end > visible_start && original_y < visible_end {
                        self.mark_dirty(*child_id);
                    }
                }
            }

            // Limit scrolling: allow scrolling until the final line appears at the top of the view region
            if formatting.overflow_y == Overflow::Scroll && !child_ids_vec.is_empty() {
                // Get the y position of the last child (children are in order)
                if let Some(last_child_id) = child_ids_vec.last() {
                    if let Some(child_rect) = self.rects.get(*last_child_id) {
                        let max_child_y = child_rect.y as usize;

                        // Max scroll is the y position of the last child
                        // This allows scrolling until the final line appears at the top
                        if parent_scroll_y > max_child_y {
                            if let Some(scroll) = self.scroll_y.get_mut(id) {
                                *scroll = max_child_y;
                            }
                        }
                    }
                }
            }

            // Track the lowest point rendered by children
            let mut lowest_rendered_y: Option<u16> = None;

            for child_id in child_ids_vec {
                // Get child rect info without holding a borrow
                let (original_y, child_height) = {
                    if let Some(child_rect) = self.rects.get(child_id) {
                        (child_rect.y as usize, child_rect.height as usize)
                    } else {
                        continue;
                    }
                };

                // Check if child is visible within the scrollable area
                // Child is visible if it overlaps with [scroll_y, scroll_y + parent_height)
                let visible_start = parent_scroll_y;
                let visible_end = parent_scroll_y + parent_rect.height as usize;
                let child_end = original_y + child_height;

                // Skip if completely above or below visible range
                if child_end <= visible_start || original_y >= visible_end {
                    continue;
                }

                // Calculate screen position: parent.y + (child.y - scroll_y)
                let screen_y = if original_y >= parent_scroll_y {
                    parent_rect.y + (original_y - parent_scroll_y) as u16
                } else {
                    // Child partially above scroll point, render at parent's top
                    parent_rect.y
                };

                // Update lowest rendered point
                let child_screen_bottom = screen_y + child_height as u16;
                lowest_rendered_y = Some(match lowest_rendered_y {
                    None => child_screen_bottom,
                    Some(current_lowest) => current_lowest.max(child_screen_bottom),
                });

                // Calculate skip_lines before adjusting position
                let skip_lines = if original_y < parent_scroll_y {
                    (parent_scroll_y - original_y) as u16
                } else {
                    0
                };

                // Temporarily adjust child's y position for rendering
                {
                    if let Some(child_rect_mut) = self.rects.get_mut(child_id) {
                        child_rect_mut.y = screen_y;
                    }
                } // Drop the mutable borrow

                // Store skip_lines override before rendering
                self.skip_lines_override.insert(child_id, skip_lines);
                let _ = self.render_node(child_id, stdout);
                self.skip_lines_override.remove(&child_id);

                // Restore original y position for next frame
                {
                    if let Some(child_rect_mut) = self.rects.get_mut(child_id) {
                        child_rect_mut.y = original_y as u16;
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
                    let current_scroll = self.scroll_y.get(self.focus).copied().unwrap_or(0);
                    let new_scroll = current_scroll.saturating_sub(1);

                    // Get scroll bounds from the focused component
                    self.scroll_y.insert(self.focus, new_scroll);

                    self.mark_dirty(self.focus);
                    return Ok(());
                }
                MouseEventKind::ScrollDown => {
                    let current_scroll = self.scroll_y.get(self.focus).copied().unwrap_or(0);

                    // Get the max scroll position from the last child, same as render_node logic
                    let mut max_scroll = current_scroll; // Default to current if no children
                    if let Some(child_ids_vec) = self.children(self.focus) {
                        if !child_ids_vec.is_empty() {
                            if let Some(last_child_id) = child_ids_vec.last() {
                                if let Some(child_rect) = self.rects.get(*last_child_id) {
                                    max_scroll = child_rect.y as usize;
                                }
                            }
                        }
                    }

                    // Clamp to prevent scrolling past the end
                    let new_scroll = (current_scroll + 1).min(max_scroll);

                    self.scroll_y.insert(self.focus, new_scroll);

                    self.mark_dirty(self.focus);
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

        Ok(())
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
