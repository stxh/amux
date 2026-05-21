/// Terminal Group — a named collection of tabs
///
/// Groups are shown in the left sidebar as icons with auto-numbering.
/// Each group contains multiple tabs, similar to tmux sessions.

use crate::pane_tree::{PaneId, SplitDirection};
use crate::tab::Tab;
use crate::terminal::TerminalConfig;

/// Icons for groups (auto-assigned based on group index — simple numbers)
const GROUP_ICONS: &[&str] = &[
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "0",
];

/// A terminal group containing multiple tabs
pub struct TerminalGroup {
    pub id: usize,
    /// Auto-assigned icon
    pub icon: String,
    /// Group name (auto-generated)
    pub name: String,
    /// Tabs in this group
    pub tabs: Vec<Tab>,
    /// Active tab index
    pub active_tab_index: usize,
}

impl TerminalGroup {
    /// Create a new group with one initial tab
    pub fn new(
        group_id: usize,
        initial_tab_id: usize,
        initial_pane_id: PaneId,
        config: &TerminalConfig,
    ) -> Self {
        let icon_idx = (group_id - 1) % GROUP_ICONS.len();
        let tab = Tab::new(initial_tab_id, initial_pane_id, config);

        Self {
            id: group_id,
            icon: GROUP_ICONS[icon_idx].to_string(),
            name: format!("Group {}", group_id),
            tabs: vec![tab],
            active_tab_index: 0,
        }
    }

    /// Get the active tab
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab_index)
    }

    /// Get the active tab mutably
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_tab_index)
    }

    /// Add a new tab to this group
    pub fn add_tab(&mut self, tab_id: usize, pane_id: PaneId, config: &TerminalConfig) {
        let tab = Tab::new(tab_id, pane_id, config);
        self.tabs.push(tab);
        self.active_tab_index = self.tabs.len() - 1;
    }

    /// Remove the active tab. Returns false if it's the last tab.
    pub fn remove_active_tab(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }

        self.tabs.remove(self.active_tab_index);
        if self.active_tab_index >= self.tabs.len() {
            self.active_tab_index = self.tabs.len() - 1;
        }
        true
    }

    /// Switch to a specific tab by index
    pub fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab_index = index;
        }
    }

    /// Switch to the next tab
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab_index = (self.active_tab_index + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab_index = if self.active_tab_index == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab_index - 1
            };
        }
    }

    /// Split the active pane in the active tab
    pub fn split_active_pane(
        &mut self,
        direction: SplitDirection,
        new_pane_id: PaneId,
        config: &TerminalConfig,
    ) -> Option<PaneId> {
        self.active_tab_mut()?.split_active(direction, new_pane_id, config)
    }

    /// Close the active pane in the active tab
    pub fn close_active_pane(&mut self) -> bool {
        if let Some(tab) = self.active_tab_mut() {
            tab.close_active_pane()
        } else {
            false
        }
    }

    /// Navigate to next pane in active tab
    pub fn next_pane(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.next_pane();
        }
    }

    /// Navigate to previous pane in active tab
    pub fn prev_pane(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.prev_pane();
        }
    }

    /// Get tab count
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }
}
