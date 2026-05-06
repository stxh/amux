use crate::group::TerminalGroup;
use crate::pane::Pane;
use crate::pane_tree::{PaneId, SplitDirection};
use crate::terminal::TerminalConfig;
use xilem::AppState;

/// Global application state
pub struct App {
    /// Terminal groups (shown in left sidebar)
    pub groups: Vec<TerminalGroup>,
    /// Active group index
    pub active_group_index: usize,
    /// Global ID counter for panes
    pub next_pane_id: PaneId,
    /// Global ID counter for tabs
    pub next_tab_id: usize,
    /// Global ID counter for groups
    pub next_group_id: usize,
    /// Terminal configuration
    pub terminal_config: TerminalConfig,
    /// Whether we're in prefix mode (waiting for a command after Ctrl+B)
    pub prefix_mode: bool,
    /// Prefix mode indicator for UI
    pub prefix_indicator: String,
    /// Status message
    pub status_message: String,
    /// Keep running flag
    pub running: bool,
    /// Redraw counter - incremented to force view rebuilds
    pub redraw_counter: u64,
    /// Pending resizes (pane_id, cols, rows)
    pub pending_resizes: Vec<(PaneId, u16, u16)>,
}

impl AppState for App {
    fn keep_running(&self) -> bool {
        self.running
    }
}

impl App {
    pub fn new() -> Self {
        let config = TerminalConfig::default();

        // Create first group with one tab and one pane
        let group = TerminalGroup::new(1, 1, 1, &config);

        Self {
            groups: vec![group],
            active_group_index: 0,
            next_pane_id: 2,   // pane 1 already used
            next_tab_id: 2,    // tab 1 already used
            next_group_id: 2,  // group 1 already used
            terminal_config: config,
            prefix_mode: false,
            prefix_indicator: String::new(),
            status_message: "Ready - Ctrl+B for prefix mode".to_string(),
            running: true,
            redraw_counter: 0,
            pending_resizes: Vec::new(),
        }
    }

    // ─── Group accessors ───

    /// Get the active group
    pub fn active_group(&self) -> Option<&TerminalGroup> {
        self.groups.get(self.active_group_index)
    }

    /// Get the active group mutably
    pub fn active_group_mut(&mut self) -> Option<&mut TerminalGroup> {
        self.groups.get_mut(self.active_group_index)
    }

    // ─── Backward-compatible pane accessors ───

    /// Get a reference to the active pane (delegates through group → tab → pane_tree)
    pub fn active_pane(&self) -> Option<&Pane> {
        self.active_group()?.active_tab()?.active_pane()
    }

    /// Get a mutable reference to the active pane
    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_group_mut()?.active_tab_mut()?.active_pane_mut()
    }

    /// Apply all pending terminal resizes
    pub fn apply_pending_resizes(&mut self) {
        let resizes = std::mem::take(&mut self.pending_resizes);
        for (pane_id, cols, rows) in resizes {
            for group in &mut self.groups {
                for tab in &mut group.tabs {
                    if let Some((pane, _)) = tab.pane_tree.find_mut(pane_id) {
                        pane.resize(cols, rows);
                    }
                }
            }
        }
    }

    // ─── Group management ───

    /// Add a new group
    pub fn add_group(&mut self) {
        let group_id = self.next_group_id;
        self.next_group_id += 1;
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let group = TerminalGroup::new(group_id, tab_id, pane_id, &self.terminal_config);
        self.groups.push(group);
        self.active_group_index = self.groups.len() - 1;
        self.status_message = format!("Created group {} ({} groups total)", group_id, self.groups.len());
    }

    /// Remove the active group (unless it's the last one)
    pub fn remove_group(&mut self) {
        if self.groups.len() <= 1 {
            self.status_message = "Cannot remove the last group".to_string();
            return;
        }
        self.groups.remove(self.active_group_index);
        if self.active_group_index >= self.groups.len() {
            self.active_group_index = self.groups.len() - 1;
        }
        self.status_message = format!("{} groups remaining", self.groups.len());
    }

    /// Switch to a specific group by index
    pub fn switch_group(&mut self, index: usize) {
        if index < self.groups.len() {
            self.active_group_index = index;
            if let Some(g) = self.active_group() {
                self.status_message = format!("Switched to {}", g.name);
            }
        }
    }

    /// Switch to next group
    pub fn next_group(&mut self) {
        if !self.groups.is_empty() {
            self.active_group_index = (self.active_group_index + 1) % self.groups.len();
            if let Some(g) = self.active_group() {
                self.status_message = format!("Switched to {}", g.name);
            }
        }
    }

    /// Switch to previous group
    pub fn prev_group(&mut self) {
        if !self.groups.is_empty() {
            self.active_group_index = if self.active_group_index == 0 {
                self.groups.len() - 1
            } else {
                self.active_group_index - 1
            };
            if let Some(g) = self.active_group() {
                self.status_message = format!("Switched to {}", g.name);
            }
        }
    }

    // ─── Tab management (delegates to active group) ───

    /// Add a new tab to the active group
    pub fn add_tab(&mut self) {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let config = self.terminal_config.clone();
        if let Some(group) = self.active_group_mut() {
            group.add_tab(tab_id, pane_id, &config);
            self.status_message = format!("New tab {} ({} tabs)", tab_id, self.groups[self.active_group_index].tab_count());
        }
    }

    /// Remove the active tab from the active group
    pub fn remove_tab(&mut self) {
        let mut should_remove_group = false;
        if let Some(group) = self.active_group_mut() {
            if group.remove_active_tab() {
                self.status_message = format!("{} tabs remaining", group.tab_count());
            } else {
                should_remove_group = true;
            }
        }
        
        if should_remove_group {
            if self.groups.len() <= 1 {
                self.status_message = "Cannot remove the last tab of the app".to_string();
            } else {
                self.remove_group();
                self.status_message = "Closed last tab and group".to_string();
            }
        }
    }

    /// Switch to a specific tab by index (1-based for user, 0-based internally)
    pub fn switch_tab(&mut self, index: usize) {
        if let Some(group) = self.active_group_mut() {
            group.switch_tab(index);
        }
    }

    /// Switch to next tab
    pub fn next_tab(&mut self) {
        if let Some(group) = self.active_group_mut() {
            group.next_tab();
        }
    }

    /// Switch to previous tab
    pub fn prev_tab(&mut self) {
        if let Some(group) = self.active_group_mut() {
            group.prev_tab();
        }
    }

    // ─── Pane management (delegates through group → tab) ───

    /// Split the active pane
    pub fn split_pane(&mut self, direction: SplitDirection) {
        let new_pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let config = self.terminal_config.clone();
        if let Some(group) = self.active_group_mut() {
            if let Some(id) = group.split_active_pane(direction, new_pane_id, &config) {
                self.status_message = format!("Split pane, new pane {}", id);
            }
        }
    }

    /// Close the active pane
    pub fn close_pane(&mut self) {
        if let Some(group) = self.active_group_mut() {
            if group.close_active_pane() {
                self.status_message = "Closed pane".to_string();
            } else {
                self.status_message = "Cannot close the last pane".to_string();
            }
        }
    }

    /// Switch to the next pane
    pub fn next_pane(&mut self) {
        if let Some(group) = self.active_group_mut() {
            group.next_pane();
            self.status_message = "Next pane".to_string();
        }
    }

    /// Switch to the previous pane
    pub fn prev_pane(&mut self) {
        if let Some(group) = self.active_group_mut() {
            group.prev_pane();
            self.status_message = "Previous pane".to_string();
        }
    }

    // ─── Key handling ───

    /// Handle key events, including prefix mode
    pub fn handle_key(&mut self, key: &str) {
        if self.prefix_mode {
            match key {
                "c" => self.split_pane(SplitDirection::Vertical),
                "d" | "\"" => self.split_pane(SplitDirection::Horizontal),
                "%" => self.split_pane(SplitDirection::Vertical),
                "n" => self.next_pane(),
                "p" => self.prev_pane(),
                "x" => self.close_pane(),
                "t" => self.add_tab(),
                "w" => self.remove_tab(),
                "g" => self.next_group(),
                _ => {
                    // Check for tab switching with number keys
                    if let Ok(num) = key.parse::<usize>() {
                        if num >= 1 && num <= 9 {
                            self.switch_tab(num - 1);
                        }
                    }
                }
            }
            self.prefix_mode = false;
        } else if key == "C-b" {
            self.prefix_mode = true;
            self.status_message = "PREFIX - waiting for command...".to_string();
        } else if let Some(pane) = self.active_pane_mut() {
            let _ = pane.terminal.write_input(key.as_bytes());
        }
    }

    /// Handle a raw keyboard event with modifier information
    pub fn handle_raw_key_event(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool) -> bool {
        crate::keyboard::handle_key_input(self, key, ctrl, alt, shift)
    }
}
