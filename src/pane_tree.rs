/// Recursive pane tree — supports nested horizontal/vertical splits
///
/// Each Tab contains one PaneTree. A PaneTree is either:
/// - A Leaf containing a single Pane (terminal)
/// - An HSplit (left/right) with adjustable ratio
/// - A VSplit (top/bottom) with adjustable ratio

use crate::pane::Pane;
use crate::terminal::TerminalConfig;
use crate::terminal_render::TerminalPaneState;

/// Unique identifier for a pane within the tree
pub type PaneId = usize;

/// A rectangular region in pixel coordinates
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
}

/// Split direction
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitDirection {
    Horizontal, // left | right
    Vertical,   // top / bottom
}

/// Recursive pane tree structure
pub enum PaneTree {
    /// A single terminal pane (leaf node)
    Leaf {
        pane: Pane,
        state: TerminalPaneState,
    },
    /// A split into two subtrees
    Split {
        direction: SplitDirection,
        first: Box<PaneTree>,   // left or top
        second: Box<PaneTree>,  // right or bottom
        /// Split ratio (0.0 - 1.0), proportion allocated to `first`
        ratio: f64,
    },
}

impl PaneTree {
    /// Create a new leaf node with a fresh terminal
    pub fn new_leaf(pane_id: PaneId, config: &TerminalConfig) -> Self {
        let pane = Pane::new(pane_id, config);
        PaneTree::Leaf {
            pane,
            state: TerminalPaneState::new(),
        }
    }

    /// Create a leaf with an existing pane
    pub fn from_pane(pane: Pane, state: TerminalPaneState) -> Self {
        PaneTree::Leaf { pane, state }
    }

    /// Split the pane identified by `target_id` in the given direction.
    /// Returns the new pane's id if the split was successful.
    pub fn split(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_pane_id: PaneId,
        config: &TerminalConfig,
    ) -> Option<PaneId> {
        match self {
            PaneTree::Leaf { pane, .. } if pane.id == target_id => {
                // Replace this leaf with a split node
                let old = std::mem::replace(
                    self,
                    PaneTree::new_leaf(0, config), // placeholder
                );
                let new_leaf = PaneTree::new_leaf(new_pane_id, config);

                *self = PaneTree::Split {
                    direction,
                    first: Box::new(old),
                    second: Box::new(new_leaf),
                    ratio: 0.5,
                };
                Some(new_pane_id)
            }
            PaneTree::Leaf { .. } => None,
            PaneTree::Split { first, second, .. } => {
                first.split(target_id, direction, new_pane_id, config)
                    .or_else(|| second.split(target_id, direction, new_pane_id, config))
            }
        }
    }

    /// Remove the pane identified by `target_id`.
    /// Returns true if removal was successful.
    /// The tree collapses: the sibling takes over the parent's position.
    pub fn remove(&mut self, target_id: PaneId) -> bool {
        match self {
            PaneTree::Leaf { pane, .. } => {
                // Can't remove the only leaf from outside
                pane.id == target_id
            }
            PaneTree::Split { first, second, .. } => {
                // Check if first child is the target leaf
                if let PaneTree::Leaf { pane, .. } = first.as_ref() {
                    if pane.id == target_id {
                        // Replace self with second
                        let sibling = std::mem::replace(
                            second.as_mut(),
                            PaneTree::new_leaf(0, &TerminalConfig::default()),
                        );
                        *self = sibling;
                        return true;
                    }
                }
                // Check if second child is the target leaf
                if let PaneTree::Leaf { pane, .. } = second.as_ref() {
                    if pane.id == target_id {
                        // Replace self with first
                        let sibling = std::mem::replace(
                            first.as_mut(),
                            PaneTree::new_leaf(0, &TerminalConfig::default()),
                        );
                        *self = sibling;
                        return true;
                    }
                }
                // Recurse into children
                first.remove(target_id) || second.remove(target_id)
            }
        }
    }

    /// Find a pane by id and return a reference
    pub fn find(&self, target_id: PaneId) -> Option<(&Pane, &TerminalPaneState)> {
        match self {
            PaneTree::Leaf { pane, state } if pane.id == target_id => Some((pane, state)),
            PaneTree::Leaf { .. } => None,
            PaneTree::Split { first, second, .. } => {
                first.find(target_id).or_else(|| second.find(target_id))
            }
        }
    }

    /// Find a pane by id and return a mutable reference
    pub fn find_mut(&mut self, target_id: PaneId) -> Option<(&mut Pane, &mut TerminalPaneState)> {
        match self {
            PaneTree::Leaf { pane, state } if pane.id == target_id => Some((pane, state)),
            PaneTree::Leaf { .. } => None,
            PaneTree::Split { first, second, .. } => {
                first.find_mut(target_id).or_else(|| second.find_mut(target_id))
            }
        }
    }

    /// Collect all pane ids in order (depth-first, left/top first)
    pub fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            PaneTree::Leaf { pane, .. } => vec![pane.id],
            PaneTree::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    /// Count the total number of leaf panes
    pub fn pane_count(&self) -> usize {
        match self {
            PaneTree::Leaf { .. } => 1,
            PaneTree::Split { first, second, .. } => {
                first.pane_count() + second.pane_count()
            }
        }
    }

    /// Get the first leaf pane id
    pub fn first_pane_id(&self) -> PaneId {
        match self {
            PaneTree::Leaf { pane, .. } => pane.id,
            PaneTree::Split { first, .. } => first.first_pane_id(),
        }
    }

    /// Get the next pane id after the given one (wrapping)
    pub fn next_pane_id(&self, current_id: PaneId) -> PaneId {
        let ids = self.pane_ids();
        if ids.is_empty() {
            return current_id;
        }
        let pos = ids.iter().position(|&id| id == current_id).unwrap_or(0);
        ids[(pos + 1) % ids.len()]
    }

    /// Get the previous pane id before the given one (wrapping)
    pub fn prev_pane_id(&self, current_id: PaneId) -> PaneId {
        let ids = self.pane_ids();
        if ids.is_empty() {
            return current_id;
        }
        let pos = ids.iter().position(|&id| id == current_id).unwrap_or(0);
        if pos == 0 {
            ids[ids.len() - 1]
        } else {
            ids[pos - 1]
        }
    }

    /// Calculate layout rectangles for each pane given an available area.
    /// Returns a list of (PaneId, Rect) pairs.
    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            PaneTree::Leaf { pane, .. } => {
                vec![(pane.id, area)]
            }
            PaneTree::Split { direction, first, second, ratio, .. } => {
                let (first_area, second_area) = match direction {
                    SplitDirection::Horizontal => {
                        let first_w = area.width * ratio;
                        let second_w = area.width - first_w;
                        (
                            Rect::new(area.x, area.y, first_w, area.height),
                            Rect::new(area.x + first_w, area.y, second_w, area.height),
                        )
                    }
                    SplitDirection::Vertical => {
                        let first_h = area.height * ratio;
                        let second_h = area.height - first_h;
                        (
                            Rect::new(area.x, area.y, area.width, first_h),
                            Rect::new(area.x, area.y + first_h, area.width, second_h),
                        )
                    }
                };
                let mut result = first.layout(first_area);
                result.extend(second.layout(second_area));
                result
            }
        }
    }

    /// Visit all leaf panes with a callback
    pub fn for_each<F: FnMut(&Pane, &TerminalPaneState)>(&self, f: &mut F) {
        match self {
            PaneTree::Leaf { pane, state } => f(pane, state),
            PaneTree::Split { first, second, .. } => {
                first.for_each(f);
                second.for_each(f);
            }
        }
    }

    /// Visit all leaf panes mutably
    pub fn for_each_mut<F: FnMut(&mut Pane, &mut TerminalPaneState)>(&mut self, f: &mut F) {
        match self {
            PaneTree::Leaf { pane, state } => f(pane, state),
            PaneTree::Split { first, second, .. } => {
                first.for_each_mut(f);
                second.for_each_mut(f);
            }
        }
    }

    /// Find the split node containing the given pane and adjust its ratio.
    /// `delta` is added to the current ratio, clamped to [0.1, 0.9].
    pub fn adjust_ratio(&mut self, pane_id: PaneId, delta: f64) -> bool {
        match self {
            PaneTree::Leaf { .. } => false,
            PaneTree::Split { first, second, ratio, .. } => {
                // Check if this split directly contains the target pane
                let first_ids = first.pane_ids();
                let second_ids = second.pane_ids();
                if first_ids.contains(&pane_id) || second_ids.contains(&pane_id) {
                    // Only adjust if the pane is a direct child (leaf)
                    let is_direct = matches!(first.as_ref(), PaneTree::Leaf { pane, .. } if pane.id == pane_id)
                        || matches!(second.as_ref(), PaneTree::Leaf { pane, .. } if pane.id == pane_id);
                    if is_direct {
                        *ratio = (*ratio + delta).clamp(0.1, 0.9);
                        return true;
                    }
                }
                // Recurse
                first.adjust_ratio(pane_id, delta) || second.adjust_ratio(pane_id, delta)
            }
        }
    }
}
