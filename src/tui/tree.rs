use crate::event::ReovimEvent;
use crate::tui::debug::DebugComponent;
use crate::tui::gutter::GutterComponent;
use crate::tui::status::StatusComponent;
use crate::tui::terminal_buffer::{TerminalBuffer, TerminalCommand};
use crate::tui::text::TextComponent;
use crate::tui::{Component, Formatting, Measurement, Overflow, Rect};
use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::cursor::MoveTo;
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
    pub fn render(&self, stdout: &mut Stdout) -> Result<()> {
        self.render_node(self.root, stdout)?;
        Ok(())
    }

    fn render_node(&self, id: ComponentId, stdout: &mut Stdout) -> Result<()> {
        if let Some(component) = self.components.get(id) {
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

            // Create a virtual buffer for this component
            let mut buffer = TerminalBuffer::new(rect.width, rect.height);

            // Set focus if this component is focused
            if id == self.focus {
                buffer.set_focus(true);
            }

            // Render component into the virtual buffer
            if self.is_dirty(id) {
                component.render(&mut buffer)?;
                stdout.execute(ResetColor)?;
                // Choose compositing method based on overflow settings
                match formatting.overflow_x {
                    Overflow::Wrap => {
                        self.composite_buffer_wrap(stdout, &buffer, rect.x, rect.y)?;
                    }
                    Overflow::Hide => {
                        self.composite_buffer_hide(stdout, &buffer, rect.x, rect.y)?;
                    }
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
        // For now, just pass to root
        // In the future: focus system, event routing, etc.
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
