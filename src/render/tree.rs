use anyhow::*;
use std::io::Stdout;

use crate::render::element::Element;

type ElementId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

struct Container {}

impl Container {
    fn new() -> Self {
        Self {}
    }
}

pub enum ElementNode {
    Container(Container),
    Component(Box<dyn Element>),
}
pub struct ElementTree {
    root: ElementId,
    elements: Vec<ElementNode>,
    parent: Vec<Option<ElementId>>,
    children: Vec<Vec<ElementId>>,
    region: Vec<Region>,
}

impl ElementTree {
    fn layout(&mut self, width: u16, height: u16) -> Result<()> {
        Ok(())
    }
    fn render(&self, stdout: Stdout) -> Result<()> {
        Ok(())
    }
}
