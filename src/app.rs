use crate::pane::{LayoutMode, Pane, PaneLayout};
use crate::terminal::TerminalConfig;
use crate::terminal_render::TerminalPaneState;
use xilem::AppState;

/// Global application state
pub struct App {
    pub panes: Vec<Pane>,
    pub pane_states: Vec<TerminalPaneState>,
    pub active_pane_index: usize,
    pub next_pane_id: usize,
    pub terminal_config: TerminalConfig,
    /// Whether we're in prefix mode (waiting for a command after Ctrl+B)
    pub prefix_mode: bool,
    /// Prefix mode indicator for UI
    pub prefix_indicator: String,
    /// Status message
    pub status_message: String,
    /// Keep running flag
    pub running: bool,
}

impl AppState for App {
    fn keep_running(&self) -> bool {
        self.running
    }
}

impl App {
    pub fn new() -> Self {
        let config = TerminalConfig::default();
        let mut first_pane = Pane::new(1, &config);
        first_pane.set_active(true);
        
        // Set initial layout: single pane fills entire area
        first_pane.layout = PaneLayout {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        
        // Spawn the shell for the first pane (PowerShell on Windows)
        if let Err(e) = first_pane.terminal.spawn_shell() {
            eprintln!("Failed to spawn shell: {}", e);
        }

        Self {
            panes: vec![first_pane],
            pane_states: vec![TerminalPaneState::new()],
            active_pane_index: 0,
            next_pane_id: 2,
            terminal_config: config,
            prefix_mode: false,
            prefix_indicator: "".to_string(),
            status_message: "Ready - Press + to split vertically, - to split horizontally".to_string(),
            running: true,
        }
    }

    /// Get a reference to the active pane
    pub fn active_pane(&self) -> Option<&Pane> {
        self.panes.get(self.active_pane_index)
    }

    /// Get a mutable reference to the active pane
    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(self.active_pane_index)
    }

    /// Split the active pane horizontally or vertically
    pub fn split_pane(&mut self, mode: LayoutMode) {
        let Some(active) = self.active_pane() else { return };
        let old_layout = active.layout;

        let new_id = self.next_pane_id;
        self.next_pane_id += 1;

        let mut new_pane = Pane::new(new_id, &self.terminal_config);
        // Spawn shell for new pane
        if let Err(e) = new_pane.terminal.spawn_shell() {
            eprintln!("Failed to spawn shell for pane {}: {}", new_id, e);
        }
        new_pane.set_active(true);

        // Deactivate the old pane
        if let Some(old) = self.panes.get_mut(self.active_pane_index) {
            old.set_active(false);
        }

        // Calculate new layouts based on split mode
        match mode {
            LayoutMode::Horizontal => {
                // Side by side
                self.panes[self.active_pane_index].layout = PaneLayout {
                    x: old_layout.x,
                    y: old_layout.y,
                    width: old_layout.width / 2.0,
                    height: old_layout.height,
                };
                new_pane.layout = PaneLayout {
                    x: old_layout.x + old_layout.width / 2.0,
                    y: old_layout.y,
                    width: old_layout.width / 2.0,
                    height: old_layout.height,
                };
            }
            LayoutMode::Vertical => {
                // Top and bottom
                self.panes[self.active_pane_index].layout = PaneLayout {
                    x: old_layout.x,
                    y: old_layout.y,
                    width: old_layout.width,
                    height: old_layout.height / 2.0,
                };
                new_pane.layout = PaneLayout {
                    x: old_layout.x,
                    y: old_layout.y + old_layout.height / 2.0,
                    width: old_layout.width,
                    height: old_layout.height / 2.0,
                };
            }
        }

        self.panes.push(new_pane);
        self.pane_states.push(TerminalPaneState::new());
        self.active_pane_index = self.panes.len() - 1;
        self.status_message = format!("Split pane, now {} panes", self.panes.len());
    }

    /// Close the active pane
    pub fn close_pane(&mut self) {
        if self.panes.len() <= 1 {
            return; // Don't close the last pane
        }

        self.panes.remove(self.active_pane_index);
        self.pane_states.remove(self.active_pane_index);
        if self.active_pane_index >= self.panes.len() {
            self.active_pane_index = self.panes.len() - 1;
        }

        // Activate the new current pane
        if let Some(pane) = self.panes.get_mut(self.active_pane_index) {
            pane.set_active(true);
        }
    }

    /// Switch to the next pane
    pub fn next_pane(&mut self) {
        if self.panes.len() <= 1 {
            return;
        }

        if let Some(current) = self.panes.get_mut(self.active_pane_index) {
            current.set_active(false);
        }

        self.active_pane_index = (self.active_pane_index + 1) % self.panes.len();

        if let Some(next) = self.panes.get_mut(self.active_pane_index) {
            next.set_active(true);
        }
    }

    /// Switch to the previous pane
    pub fn prev_pane(&mut self) {
        if self.panes.len() <= 1 {
            return;
        }

        if let Some(current) = self.panes.get_mut(self.active_pane_index) {
            current.set_active(false);
        }

        self.active_pane_index = if self.active_pane_index == 0 {
            self.panes.len() - 1
        } else {
            self.active_pane_index - 1
        };

        if let Some(prev) = self.panes.get_mut(self.active_pane_index) {
            prev.set_active(true);
        }
    }

    /// Handle key events, including prefix mode
    pub fn handle_key(&mut self, key: &str) {
        if self.prefix_mode {
            // Handle prefix mode commands
            match key {
                "c" => self.split_pane(LayoutMode::Vertical),
                "d" => self.split_pane(LayoutMode::Horizontal),
                "n" => self.next_pane(),
                "p" => self.prev_pane(),
                "x" => self.close_pane(),
                _ => {}
            }
            self.prefix_mode = false;
        } else {
            // Check for prefix key (Ctrl+B)
            if key == "C-b" {
                self.prefix_mode = true;
            } else if let Some(pane) = self.active_pane_mut() {
                // Forward to the active terminal
                let _ = pane.terminal.write_input(key.as_bytes());
            }
        }
    }
}
