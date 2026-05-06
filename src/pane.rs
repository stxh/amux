use crate::terminal::{Terminal, TerminalConfig};

/// Represents a single terminal pane (leaf node of PaneTree)
pub struct Pane {
    pub id: usize,
    pub terminal: Terminal,
    pub is_active: bool,
}

impl Pane {
    pub fn new(id: usize, config: &TerminalConfig) -> Self {
        Self {
            id,
            terminal: Terminal::new(config),
            is_active: false,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.terminal.resize(cols, rows);
    }
}
