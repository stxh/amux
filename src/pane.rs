use crate::terminal::{Terminal, TerminalConfig};

/// Represents a single terminal pane in the multiplexer
pub struct Pane {
    pub id: usize,
    pub terminal: Terminal,
    pub is_active: bool,
    /// Position and size within the layout (x, y, width, height)
    pub layout: PaneLayout,
}

#[derive(Clone, Copy, Debug)]
pub struct PaneLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for PaneLayout {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl Pane {
    pub fn new(id: usize, config: &TerminalConfig) -> Self {
        Self {
            id,
            terminal: Terminal::new(config),
            is_active: false,
            layout: PaneLayout::default(),
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.terminal.resize(cols, rows);
    }
}

/// Layout mode for tiling
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutMode {
    /// Horizontal split (side by side)
    Horizontal,
    /// Vertical split (top/bottom)
    Vertical,
}
