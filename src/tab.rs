/// Tab — contains a named terminal workspace with a pane tree
///
/// Each Tab holds one PaneTree which can be recursively split.
/// Tab names auto-track the active shell's working directory (tmux-style).

use crate::pane_tree::{PaneId, PaneTree, SplitDirection};
use crate::terminal::TerminalConfig;

/// A single tab within a terminal group
pub struct Tab {
    /// The recursive pane tree
    pub pane_tree: PaneTree,
    /// Currently focused pane id within this tab
    pub active_pane_id: PaneId,
}

impl Tab {
    /// Create a new tab with a single terminal pane
    pub fn new(pane_id: PaneId, config: &TerminalConfig) -> Self {
        let mut pane_tree = PaneTree::new_leaf(pane_id, config);
        // Spawn shell for the initial pane
        if let Some((pane, _)) = pane_tree.find_mut(pane_id) {
            if let Err(e) = pane.terminal.spawn_shell() {
                eprintln!("Failed to spawn shell for pane {}: {}", pane_id, e);
            }
            pane.set_active(true);
        }

        Self {
            pane_tree,
            active_pane_id: pane_id,
        }
    }

    /// Split the active pane in the given direction
    /// Returns the new pane id if successful
    pub fn split_active(
        &mut self,
        direction: SplitDirection,
        new_pane_id: PaneId,
        config: &TerminalConfig,
    ) -> Option<PaneId> {
        let result = self.pane_tree.split(
            self.active_pane_id,
            direction,
            new_pane_id,
            config,
        );

        if result.is_some() {
            // Spawn shell for the new pane
            if let Some((pane, _)) = self.pane_tree.find_mut(new_pane_id) {
                if let Err(e) = pane.terminal.spawn_shell() {
                    eprintln!("Failed to spawn shell for pane {}: {}", new_pane_id, e);
                }
            }
            // Deactivate old pane, activate new
            if let Some((old_pane, _)) = self.pane_tree.find_mut(self.active_pane_id) {
                old_pane.set_active(false);
            }
            if let Some((new_pane, _)) = self.pane_tree.find_mut(new_pane_id) {
                new_pane.set_active(true);
            }
            self.active_pane_id = new_pane_id;
        }

        result
    }

    /// Close the active pane. Returns false if it's the last pane.
    pub fn close_active_pane(&mut self) -> bool {
        if self.pane_tree.pane_count() <= 1 {
            return false;
        }

        let next_id = self.pane_tree.next_pane_id(self.active_pane_id);
        let old_id = self.active_pane_id;

        if self.pane_tree.remove(old_id) {
            // Activate the next pane
            self.active_pane_id = if self.pane_tree.find(next_id).is_some() {
                next_id
            } else {
                self.pane_tree.first_pane_id()
            };
            if let Some((pane, _)) = self.pane_tree.find_mut(self.active_pane_id) {
                pane.set_active(true);
            }
            true
        } else {
            false
        }
    }

    /// Switch to the next pane
    pub fn next_pane(&mut self) {
        if let Some((old, _)) = self.pane_tree.find_mut(self.active_pane_id) {
            old.set_active(false);
        }
        self.active_pane_id = self.pane_tree.next_pane_id(self.active_pane_id);
        if let Some((new, _)) = self.pane_tree.find_mut(self.active_pane_id) {
            new.set_active(true);
        }
    }

    /// Switch to the previous pane
    pub fn prev_pane(&mut self) {
        if let Some((old, _)) = self.pane_tree.find_mut(self.active_pane_id) {
            old.set_active(false);
        }
        self.active_pane_id = self.pane_tree.prev_pane_id(self.active_pane_id);
        if let Some((new, _)) = self.pane_tree.find_mut(self.active_pane_id) {
            new.set_active(true);
        }
    }

    /// Get reference to the active pane
    pub fn active_pane(&self) -> Option<&crate::pane::Pane> {
        self.pane_tree.find(self.active_pane_id).map(|(p, _)| p)
    }

    /// Get mutable reference to the active pane
    pub fn active_pane_mut(&mut self) -> Option<&mut crate::pane::Pane> {
        self.pane_tree.find_mut(self.active_pane_id).map(|(p, _)| p)
    }
}
