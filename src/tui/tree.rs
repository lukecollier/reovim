use crate::event::ReovimEvent;
use crate::tui::debug::DebugComponent;
use crate::tui::gutter::GutterComponent;
use crate::tui::status::StatusComponent;
use crate::tui::terminal_buffer::{TerminalBuffer, TerminalCommand};
use crate::tui::text::TextComponent;
use crate::tui::{Component, Formatting, Measurement, Overflow, Rect};
use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::cursor::{MoveTo, Hide, Show};
use crossterm::style::{Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use std::io::Stdout;
use tracing::debug;

pub type ComponentId = usize;

#[derive(Debug, Clone, Copy)]
pub enum LayoutMode {
    VerticalSplit,
    HorizontalSplit,
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
        self.tree.focus == self.self_id
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

    /// Set cursor position (col, row) for the component
    pub fn set_cursor(&mut self, col: u16, row: u16) {
        if let Some(col_slot) = self.tree.cursor_col.get_mut(self.self_id) {
            *col_slot = col;
        }
        if let Some(row_slot) = self.tree.cursor_row.get_mut(self.self_id) {
            *row_slot = row;
        }
        self.tree.mark_dirty(self.self_id);
    }

    /// Move cursor by the given offset (col_delta, row_delta)
    pub fn move_cursor(&mut self, col_delta: i32, row_delta: i32) {
        if let Some(col_slot) = self.tree.cursor_col.get_mut(self.self_id) {
            if col_delta < 0 {
                *col_slot = col_slot.saturating_sub((-col_delta) as u16);
            } else {
                *col_slot = col_slot.saturating_add(col_delta as u16);
            }
        }

        if let Some(row_slot) = self.tree.cursor_row.get_mut(self.self_id) {
            if row_delta < 0 {
                *row_slot = row_slot.saturating_sub((-row_delta) as u16);
            } else {
                *row_slot = row_slot.saturating_add(row_delta as u16);
            }
        }

        self.tree.mark_dirty(self.self_id);
    }

    /// Get current cursor position (col, row)
    pub fn get_cursor(&self) -> (u16, u16) {
        let col = self.tree.cursor_col.get(self.self_id).copied().unwrap_or(0);
        let row = self.tree.cursor_row.get(self.self_id).copied().unwrap_or(0);
        (col, row)
    }
}

/// A frame is a layout container that holds children
pub struct Frame {
    pub children: Vec<ComponentId>,
    pub layout_mode: LayoutMode,
}

impl Frame {
    pub fn new(layout_mode: LayoutMode) -> Self {
        Self {
            children: Vec::new(),
            layout_mode,
        }
    }
}

/// All possible component types in the arena
pub enum ComponentNode<'a> {
    Frame(Frame),
    Status(StatusComponent<'a>),
    Text(TextComponent<'a>),
    Gutter(GutterComponent),
    Debug(DebugComponent),
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
            ComponentNode::Gutter(component) => component.render(buffer),
            ComponentNode::Debug(component) => component.render(buffer),
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
            ComponentNode::Gutter(component) => component.update(event, commands),
            ComponentNode::Debug(component) => component.update(event, commands),
        }
    }

    pub fn scroll_bounds(&self) -> (usize, usize) {
        match self {
            ComponentNode::Frame(_) => (0, usize::MAX),
            ComponentNode::Status(component) => component.scroll_bounds(),
            ComponentNode::Text(component) => component.scroll_bounds(),
            ComponentNode::Gutter(component) => component.scroll_bounds(),
            ComponentNode::Debug(component) => component.scroll_bounds(),
        }
    }

    pub fn children(&self) -> Option<&[ComponentId]> {
        match self {
            ComponentNode::Frame(frame) => Some(&frame.children),
            _ => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<ComponentId>> {
        match self {
            ComponentNode::Frame(frame) => Some(&mut frame.children),
            _ => None,
        }
    }
}

/// Arena-based component tree
/// Stores all components in a flat vector and references them by index
pub struct ComponentTree<'a> {
    components: Vec<ComponentNode<'a>>,
    /// parent[i] is the parent of component i
    parent: Vec<Option<ComponentId>>,
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
}

impl<'a> ComponentTree<'a> {
    pub fn new(root: ComponentNode<'a>) -> Self {
        Self {
            components: vec![root],
            focus: 0,
            parent: vec![None],
            rects: vec![Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            }],
            formatting: vec![Formatting::default()],
            root: 0,
            dirty: vec![0],
            cursor_pos: None,
            scroll_x: vec![0],
            scroll_y: vec![0],
            cursor_col: vec![0],
            cursor_row: vec![0],
            last_cursor_row: vec![0],
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
            ComponentNode::Gutter(component) => component.default_formatting(),
            ComponentNode::Debug(component) => component.default_formatting(),
        };
        self.add_child_with_formatting(parent_id, child, formatting)
    }

    /// Add a component as a child of a parent with custom formatting
    /// Automatically adds any child nodes returned by the component's child_nodes() method
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
        self.cursor_col.push(0);
        self.cursor_row.push(0);
        self.last_cursor_row.push(0);

        // Mark as dirty so it renders on first frame (but not Frames, which shouldn't clear)
        if !is_frame {
            self.mark_dirty(child_id);
        }

        // Set focus if this component requests it
        if formatting.request_focus {
            self.focus = child_id;
        }

        // Add child to parent's children list
        if let Some(parent) = self.components.get_mut(parent_id) {
            if let Some(children) = parent.children_mut() {
                children.push(child_id);
            }
        }

        // Get child nodes after adding the component to the tree
        let child_nodes = match self.components.get(child_id) {
            Some(ComponentNode::Frame(_)) => vec![],
            Some(ComponentNode::Status(component)) => component.child_nodes(),
            Some(ComponentNode::Text(component)) => component.child_nodes(),
            Some(ComponentNode::Gutter(component)) => component.child_nodes(),
            Some(ComponentNode::Debug(component)) => component.child_nodes(),
            None => vec![],
        };

        // Recursively add child nodes
        // We do this in a separate scope to ensure we don't hold any references
        for child_node in child_nodes {
            self.add_child(child_id, child_node)?;
        }

        Ok(child_id)
    }

    /// Get a component by ID (immutable)
    pub fn get(&self, id: ComponentId) -> Option<&ComponentNode> {
        self.components.get(id)
    }

    /// Get a component by ID (mutable)
    pub fn get_mut(&mut self, id: ComponentId) -> Option<&mut ComponentNode<'a>> {
        self.components.get_mut(id)
    }

    /// Get children of a component
    pub fn children(&self, id: ComponentId) -> Option<Vec<ComponentId>> {
        self.components
            .get(id)
            .and_then(|c| c.children())
            .map(|s| s.to_vec())
    }

    /// Get parent of a component
    pub fn parent(&self, id: ComponentId) -> Option<Option<ComponentId>> {
        self.parent.get(id).copied()
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
            Measurement::Fill => available,
        }
    }

    pub fn layout(&mut self, width: u16, height: u16) {
        self.layout_node(self.root, width, height);
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
            // Get layout mode if this is a Frame
            let layout_mode = if let Some(ComponentNode::Frame(frame)) = self.components.get(id) {
                frame.layout_mode
            } else {
                LayoutMode::HorizontalSplit
            };
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
            let component_height =
                self.calculate_size(formatting.preferred_height, available_height);

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
            let component_width = self.calculate_size(formatting.preferred_width, available_width);
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
            stdout.execute(MoveTo(x, y))?;
            stdout.execute(Show)?;
        }

        Ok(())
    }

    fn render_node(&mut self, id: ComponentId, stdout: &mut Stdout) -> Result<()> {
        let rect = self.rects.get(id).copied().unwrap_or_default();
        let formatting = self.formatting.get(id).copied().unwrap_or_default();

        debug!(
            "render_node id={}: rect=({},{}) {}x{} dirty={}",
            id,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            self.is_dirty(id)
        );

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
                let mut new_scroll_y = scroll_y;

                if cursor_tree_row < scroll_y as u16 {
                    // Cursor is above visible range, scroll up to show it
                    new_scroll_y = cursor_tree_row as usize;
                } else if cursor_tree_row >= (scroll_y + rect.height as usize) as u16 {
                    // Cursor is below visible range, scroll down to show it at bottom
                    let visible_height = rect.height as usize;
                    new_scroll_y = (cursor_tree_row as usize).saturating_sub(visible_height - 1);
                }

                // Update scroll and mark as dirty for re-render
                if new_scroll_y != scroll_y {
                    final_scroll_y = new_scroll_y;
                    if let Some(scroll) = self.scroll_y.get_mut(id) {
                        *scroll = new_scroll_y;
                    }
                    self.mark_dirty(id);
                }
            }

            // Clamp cursor to visible range (for mouse scrolling)
            if formatting.overflow_y == Overflow::Scroll {
                let visible_height = rect.height as usize;
                let mut new_cursor_row = cursor_tree_row;

                // If cursor is above visible range, move it to the top
                if cursor_tree_row < final_scroll_y as u16 {
                    new_cursor_row = final_scroll_y as u16;
                    if let Some(cursor) = self.cursor_row.get_mut(id) {
                        *cursor = new_cursor_row;
                    }
                    self.mark_dirty(id);
                } else if cursor_tree_row >= (final_scroll_y + visible_height) as u16 {
                    // If cursor is below visible range, move it to the bottom
                    new_cursor_row = (final_scroll_y + visible_height.saturating_sub(1)) as u16;
                    if let Some(cursor) = self.cursor_row.get_mut(id) {
                        *cursor = new_cursor_row;
                    }
                    self.mark_dirty(id);
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
            let relative_col = cursor_tree_col;

            // Set cursor position in buffer as component-relative
            buffer.set_cursor_position(relative_col, relative_row as u16);

            // Render component into the virtual buffer
            if self.is_dirty(id) {
                component.render(&mut buffer)?;
                stdout.execute(ResetColor)?;
                // Choose compositing method based on overflow settings
                match formatting.overflow_x {
                    Overflow::Wrap => {
                        self.composite_buffer_wrap(stdout, &buffer, rect.x, rect.y)?;
                    }
                    Overflow::Hide | Overflow::Scroll => {
                        self.composite_buffer_hide(stdout, &buffer, rect.x, rect.y)?;
                    }
                }
            }

            // Store final screen cursor position for terminal output
            if id == self.focus && relative_row < rect.height as usize {
                let cursor_col = rect.x + relative_col;
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
        if let Some(child_ids) = self.children(id) {
            for child_id in child_ids {
                self.render_node(child_id, stdout)?;
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
    ) -> Result<()> {
        let mut x = 0u16;
        let mut y = 0u16;

        for cmd in buffer.commands() {
            // Early exit if we've exceeded vertical bounds
            if y >= buffer.height() {
                break;
            }

            match cmd {
                TerminalCommand::Print(ch) => {
                    if x >= buffer.width() {
                        // Skip this character (hide overflow)
                        continue;
                    }

                    // Move to position and print
                    stdout.execute(MoveTo(start_x + x, start_y + y))?;
                    stdout.execute(Print(ch))?;
                    x += 1;
                }
                TerminalCommand::Newline => {
                    // Reset colors before newline
                    stdout.execute(ResetColor)?;
                    // Pad the rest of the line with spaces
                    while x < buffer.width() {
                        stdout.execute(MoveTo(start_x + x, start_y + y))?;
                        stdout.execute(Print(' '))?;
                        x += 1;
                    }
                    x = 0;
                    y += 1;
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

        // Reset colors before padding the rest of the current row if incomplete
        stdout.execute(ResetColor)?;
        // Pad the rest of the current row if incomplete
        while x < buffer.width() {
            stdout.execute(MoveTo(start_x + x, start_y + y))?;
            stdout.execute(Print(' '))?;
            x += 1;
        }

        // Clear any remaining rows beyond what was rendered
        y += 1;
        while y < buffer.height() {
            for x_pos in 0..buffer.width() {
                stdout.execute(MoveTo(start_x + x_pos, start_y + y))?;
                stdout.execute(Print(' '))?;
            }
            y += 1;
        }

        Ok(())
    }

    fn composite_buffer_wrap(
        &self,
        stdout: &mut Stdout,
        buffer: &TerminalBuffer,
        start_x: u16,
        start_y: u16,
    ) -> Result<()> {
        let mut x = 0u16;
        let mut y = 0u16;

        for cmd in buffer.commands() {
            // Early exit if we've exceeded vertical bounds
            if y >= buffer.height() {
                break;
            }

            match cmd {
                TerminalCommand::Print(ch) => {
                    // Wrap to next line if we overflow width
                    if x >= buffer.width() {
                        x = 0;
                        y += 1;
                        if y >= buffer.height() {
                            break;
                        }
                    }

                    // Move to position and print
                    stdout.execute(MoveTo(start_x + x, start_y + y))?;
                    stdout.execute(Print(ch))?;
                    x += 1;
                }
                TerminalCommand::Newline => {
                    // Pad the rest of the line with spaces
                    while x < buffer.width() {
                        stdout.execute(MoveTo(start_x + x, start_y + y))?;
                        stdout.execute(Print(' '))?;
                        x += 1;
                    }
                    stdout.execute(ResetColor)?;
                    x = 0;
                    y += 1;
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

        // Pad the rest of the current row if incomplete
        while x < buffer.width() {
            stdout.execute(MoveTo(start_x + x, start_y + y))?;
            stdout.execute(Print(' '))?;
            x += 1;
        }
        stdout.execute(ResetColor)?;

        // Clear any remaining rows beyond what was rendered
        y += 1;
        while y < buffer.height() {
            for x_pos in 0..buffer.width() {
                stdout.execute(MoveTo(start_x + x_pos, start_y + y))?;
                stdout.execute(Print(' '))?;
            }
            y += 1;
        }

        Ok(())
    }

    /// Handle an event for the entire tree
    pub fn update(&mut self, event: ReovimEvent) -> Result<()> {
        // Handle scroll events at tree level
        if let ReovimEvent::Mouse(mouse_event) = &event {
            match mouse_event.kind {
                crossterm::event::MouseEventKind::ScrollUp => {
                    let current_scroll = self.scroll_y.get(self.focus).copied().unwrap_or(0);
                    let new_scroll = current_scroll.saturating_sub(1);

                    // Get scroll bounds from the focused component
                    if let Some(component) = self.components.get(self.focus) {
                        let (min_scroll, max_scroll) = component.scroll_bounds();
                        let clamped_scroll = new_scroll.max(min_scroll).min(max_scroll);
                        self.scroll_y.insert(self.focus, clamped_scroll);
                    } else {
                        self.scroll_y.insert(self.focus, new_scroll);
                    }

                    self.mark_dirty(self.focus);
                    return Ok(());
                }
                crossterm::event::MouseEventKind::ScrollDown => {
                    let current_scroll = self.scroll_y.get(self.focus).copied().unwrap_or(0);
                    let new_scroll = current_scroll + 1;

                    // Get scroll bounds from the focused component
                    if let Some(component) = self.components.get(self.focus) {
                        let (min_scroll, max_scroll) = component.scroll_bounds();
                        let clamped_scroll = new_scroll.max(min_scroll).min(max_scroll);
                        self.scroll_y.insert(self.focus, clamped_scroll);
                    } else {
                        self.scroll_y.insert(self.focus, new_scroll);
                    }

                    self.mark_dirty(self.focus);
                    return Ok(());
                }
                _ => {}
            }
        }

        // For other events, pass to root
        self.update_node(self.root, &event)?;
        Ok(())
    }

    fn mark_dirty(&mut self, id: ComponentId) {
        if !self.dirty.contains(&id) {
            debug!("mark_dirty: id={}", id);
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
