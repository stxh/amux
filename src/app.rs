use crate::config::Config;
use crate::terminal_render::PANE_MARGIN;
use crate::group::TerminalGroup;
use crate::pane::Pane;
use crate::pane_tree::{PaneId, SplitDirection};
use crate::terminal::TerminalConfig;
use xilem::AppState;
use copypasta::{ClipboardContext, ClipboardProvider};

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
    /// Last captured key info for debugging
    pub last_key_info: String,
    /// Redraw proxy to wake up the event loop directly
    pub redraw_proxy: Option<xilem::core::MessageProxy<()>>,
    /// Last layout of panes (cached for mouse hit testing)
    pub last_pane_layouts: Vec<(PaneId, crate::pane_tree::Rect)>,
    /// Sidebar visibility
    pub show_sidebar: bool,
    /// Right sidebar visibility
    pub show_right_sidebar: bool,
    /// Current mouse hover coordinates relative to the terminal view
    pub hover_pos: Option<(f64, f64)>,
    /// Monospace font cell width measured during rendering
    pub cell_w: f64,
    /// Monospace font cell height measured during rendering
    pub cell_h: f64,
    /// Font size in points
    pub font_size: f64,
    /// Config reference (for saving on exit)
    pub user_config: Config,
    /// Current window inner size (updated on each render)
    pub window_size: (f64, f64),
    /// Whether the window is currently maximized
    pub window_maximized: bool,
    /// System clipboard interface
    pub clipboard: Option<ClipboardContext>,
    /// Timestamp of last mouse click (for multi-click detection)
    last_click_time: std::time::Instant,
    /// Pane ID of last click (resets click count on different pane)
    last_click_pane_id: usize,
    /// Current click count (1=click, 2=double-click, 3=triple-click)
    click_count: usize,
    /// Button held during a mouse-reporting drag (adapter btn_id), for motion reports
    mouse_down_btn: Option<u8>,
}

impl AppState for App {
    fn keep_running(&self) -> bool {
        self.running
    }
}

impl App {
    pub fn new(cfg: Config) -> Self {
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
            status_message: "Ready - Shortcuts active".to_string(),
            running: true,
            redraw_counter: 0,
            pending_resizes: Vec::new(),
            last_key_info: "None".to_string(),
            redraw_proxy: None,
            last_pane_layouts: Vec::new(),
            show_sidebar: true,
            show_right_sidebar: false,
            hover_pos: None,
            font_size: cfg.font_size,
            user_config: cfg,
            window_size: (0.0, 0.0),
            window_maximized: false,
            cell_w: 10.0,
            cell_h: 20.0,
            clipboard: ClipboardContext::new().ok(),
            last_click_time: std::time::Instant::now(),
            last_click_pane_id: 0,
            click_count: 0,
            mouse_down_btn: None,
        }
    }

    /// Register the Xilem message proxy to wake up the event loop when terminal stdout changes
    pub fn register_redraw_proxy(&mut self, proxy: xilem::core::MessageProxy<()>) {
        // println!("DEBUG: Registering redraw_proxy on App!");
        self.redraw_proxy = Some(proxy.clone());
        
        // Propagate to all existing terminals in all groups & tabs
        for group in &mut self.groups {
            for tab in &mut group.tabs {
                tab.pane_tree.for_each_mut(&mut |pane, _| {
                    *pane.terminal.redraw_notifier.lock().unwrap() = Some(proxy.clone());
                });
            }
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
        let cell_w = self.cell_w.max(1.0).ceil() as u16;
        let cell_h = self.cell_h.max(1.0).ceil() as u16;
        let resizes = std::mem::take(&mut self.pending_resizes);
        for (pane_id, cols, rows) in resizes {
            for group in &mut self.groups {
                for tab in &mut group.tabs {
                    if let Some((pane, _)) = tab.pane_tree.find_mut(pane_id) {
                        pane.resize_px(cols, rows, cell_w, cell_h);
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

        let mut group = TerminalGroup::new(group_id, tab_id, pane_id, &self.terminal_config);
        if let Some(proxy) = &self.redraw_proxy {
            for tab in &mut group.tabs {
                tab.pane_tree.for_each_mut(&mut |pane, _| {
                    *pane.terminal.redraw_notifier.lock().unwrap() = Some(proxy.clone());
                });
            }
        }
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
        let proxy = self.redraw_proxy.clone(); // Clone first to avoid borrow conflicts
        if let Some(group) = self.active_group_mut() {
            group.add_tab(tab_id, pane_id, &config);
            if let Some(p) = proxy {
                if let Some(tab) = group.tabs.last_mut() {
                    tab.pane_tree.for_each_mut(&mut |pane, _| {
                        *pane.terminal.redraw_notifier.lock().unwrap() = Some(p.clone());
                    });
                }
            }
            self.status_message = format!("New tab {} ({} tabs)", tab_id, self.groups[self.active_group_index].tab_count());
        }
    }

    /// Remove a specific tab by index
    pub fn remove_tab_at(&mut self, index: usize) {
        // println!("DEBUG: App::remove_tab_at: index={}, total_tabs={}", index, self.active_group().map_or(0, |g| g.tabs.len()));
        let mut should_remove_group = false;
        if let Some(group) = self.active_group_mut() {
            if group.tabs.len() > 1 {
                group.tabs.remove(index);
                if group.active_tab_index >= group.tabs.len() {
                    group.active_tab_index = group.tabs.len().saturating_sub(1);
                }
                self.status_message = format!("Removed tab {}", index + 1);
            } else {
                should_remove_group = true;
            }
        }
        
        if should_remove_group {
            if self.groups.len() <= 1 {
                // println!("DEBUG: Cannot remove last tab/group");
                self.status_message = "Cannot remove the last tab of the app".to_string();
            } else {
                // println!("DEBUG: Removing group because last tab closed");
                self.remove_group();
                self.status_message = "Closed last tab and group".to_string();
            }
        }
    }

    /// Remove the active tab
    pub fn remove_tab(&mut self) {
        let idx = if let Some(group) = self.active_group() {
            group.active_tab_index
        } else {
            0
        };
        self.remove_tab_at(idx);
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
        let proxy = self.redraw_proxy.clone(); // Clone first to avoid borrow conflicts
        if let Some(group) = self.active_group_mut() {
            if let Some(id) = group.split_active_pane(direction, new_pane_id, &config) {
                if let Some(p) = proxy {
                    if let Some(tab) = group.active_tab_mut() {
                        if let Some((new_pane, _)) = tab.pane_tree.find_mut(id) {
                            *new_pane.terminal.redraw_notifier.lock().unwrap() = Some(p.clone());
                        }
                    }
                }
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
        if let Some(pane) = self.active_pane_mut() {
            pane.terminal.clear_selection();
            self.redraw_counter += 1;
        }
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
                "b" => {
                    self.show_sidebar = !self.show_sidebar;
                    self.status_message = format!("Sidebar visibility: {}", self.show_sidebar);
                }
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
        if let Some(pane) = self.active_pane_mut() {
            pane.terminal.clear_selection();
            self.redraw_counter += 1;
        }
        let info = format!("Key: {} (Ctrl: {}, Alt: {}, Shift: {})", key, ctrl, alt, shift);
        // println!("DEBUG: App::handle_raw_key_event: {}", info);
        self.last_key_info = info;
        crate::keyboard::handle_key_input(self, key, ctrl, alt, shift)
    }

    /// Get current cursor position of active pane
    pub fn cursor_pos(&self) -> (usize, usize) {
        if let Some(pane) = self.active_pane() {
            let (x, y) = pane.terminal.cursor();
            (x as usize, y as usize)
        } else {
            (0, 0)
        }
    }

    /// Check if any terminal pane in the entire multiplexer needs a redraw
    pub fn check_and_clear_redraw(&self) -> bool {
        let mut needs_redraw = false;
        for group in &self.groups {
            for tab in &group.tabs {
                tab.pane_tree.for_each(&mut |pane, _| {
                    if pane.terminal.check_and_clear_redraw() {
                        needs_redraw = true;
                    }
                });
            }
        }
        needs_redraw
    }

    /// Close panes whose shell process has exited (e.g. after `exit`).
    /// Cascades to tab/group removal; quitting the app when the last one closes.
    pub fn reap_exited_panes(&mut self) {
        let mut exited: Vec<(usize, usize, PaneId)> = Vec::new();
        for (gi, group) in self.groups.iter().enumerate() {
            for (ti, tab) in group.tabs.iter().enumerate() {
                tab.pane_tree.for_each(&mut |pane, _| {
                    if pane.terminal.shell_exited() {
                        exited.push((gi, ti, pane.id));
                    }
                });
            }
        }
        if exited.is_empty() {
            return;
        }
        for (gi, ti, pane_id) in exited {
            if self.groups.get(gi).is_none() {
                continue;
            }
            self.active_group_index = gi;
            let close_tab = if let Some(tab) = self.groups[gi].tabs.get_mut(ti) {
                if tab.pane_tree.pane_count() > 1 {
                    let next_id = tab.pane_tree.next_pane_id(pane_id);
                    if tab.pane_tree.remove(pane_id) {
                        tab.active_pane_id = if tab.pane_tree.find(next_id).is_some() {
                            next_id
                        } else {
                            tab.pane_tree.first_pane_id()
                        };
                        if let Some((pane, _)) = tab.pane_tree.find_mut(tab.active_pane_id) {
                            pane.set_active(true);
                        }
                    }
                    false
                } else {
                    true
                }
            } else {
                false
            };
            if close_tab {
                if self.groups[gi].tabs.len() > 1 {
                    self.groups[gi].tabs.remove(ti);
                    if self.groups[gi].active_tab_index >= self.groups[gi].tabs.len() {
                        self.groups[gi].active_tab_index = self.groups[gi].tabs.len() - 1;
                    }
                } else if self.groups.len() > 1 {
                    self.groups.remove(gi);
                    if self.active_group_index >= self.groups.len() {
                        self.active_group_index = self.groups.len() - 1;
                    }
                } else {
                    self.running = false;
                    return;
                }
            }
        }
        self.status_message = "Closed exited pane".to_string();
        self.redraw_counter += 1;
    }

    /// Scroll the active terminal pane by delta_lines (positive = scroll up into history, negative = scroll down)
    pub fn scroll_terminal(&mut self, delta_lines: i32) {
        if let Some(pane) = self.active_pane_mut() {
            pane.terminal.scroll_viewport(delta_lines);
            // Force a redraw
            self.redraw_counter += 1;
        }
    }

    /// Rename the active group (workspace)
    pub fn rename_active_group(&mut self) {
        if let Some(group) = self.active_group_mut() {
            group.name = format!("{} 📝", group.name.trim_end_matches(" 📝"));
            self.status_message = format!("Renamed workspace to {}", group.name);
            self.redraw_counter += 1;
        }
    }

    /// Edit active group (workspace) description
    pub fn edit_active_group_description(&mut self) {
        if let Some(group) = self.active_group_mut() {
            self.status_message = format!("Workspace {}: A high-performance multiplexer workspace.", group.name);
            self.redraw_counter += 1;
        }
    }

    /// Focus a pane by its unique ID
    pub fn focus_pane_by_id(&mut self, pane_id: usize) {
        if let Some(group) = self.groups.get_mut(self.active_group_index) {
            if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                if tab.active_pane_id != pane_id {
                    // Deactivate old active pane
                    if let Some((old_pane, _)) = tab.pane_tree.find_mut(tab.active_pane_id) {
                        old_pane.set_active(false);
                    }
                    // Activate clicked pane
                    if let Some((new_pane, _)) = tab.pane_tree.find_mut(pane_id) {
                        new_pane.set_active(true);
                        tab.active_pane_id = pane_id;
                        self.status_message = format!("Focused pane {}", pane_id);
                        self.redraw_counter += 1;
                    }
                }
            }
        }
    }

    /// Identify which pane contains the given coordinates
    pub fn find_pane_at(&self, px: f64, py: f64) -> Option<(usize, crate::pane_tree::Rect)> {
        for (pane_id, rect) in &self.last_pane_layouts {
            if px >= rect.x && px <= rect.x + rect.width && py >= rect.y && py <= rect.y + rect.height {
                return Some((*pane_id, *rect));
            }
        }
        None
    }

    /// Detect multi-click based on timing and pane
    fn update_click_state(&mut self, pane_id: usize) -> usize {
        const DOUBLE_CLICK_THRESHOLD_MS: u128 = 500;
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_click_time).as_millis();
        if elapsed < DOUBLE_CLICK_THRESHOLD_MS && pane_id == self.last_click_pane_id {
            self.click_count = self.click_count.saturating_add(1).min(3);
        } else {
            self.click_count = 1;
        }
        self.last_click_time = now;
        self.last_click_pane_id = pane_id;
        self.click_count
    }

    /// Handle mouse button press down at coordinates (px, py)
    pub fn handle_mouse_down(&mut self, px: f64, py: f64, button: xilem::masonry::core::PointerButton, shift: bool, alt: bool, ctrl: bool) {
        // 1. Check if clicked a pane's top-right floating close button (if split panes are present)
        let mut clicked_close_pane_id = None;
        if let Some(group) = self.groups.get(self.active_group_index) {
            if let Some(tab) = group.tabs.get(group.active_tab_index) {
                if tab.pane_tree.pane_count() > 1 {
                    for (pane_id, rect) in &self.last_pane_layouts {
                        let cb_x_min = rect.x + rect.width - 32.0;
                        let cb_x_max = rect.x + rect.width;
                        let cb_y_min = rect.y;
                        let cb_y_max = rect.y + 32.0;
                        if px >= cb_x_min && px <= cb_x_max && py >= cb_y_min && py <= cb_y_max {
                            clicked_close_pane_id = Some(*pane_id);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(pane_id) = clicked_close_pane_id {
            self.focus_pane_by_id(pane_id);
            self.close_pane();
            return;
        }

        // 2. Normal pane focus & selection/paste click
        let clicked = self.find_pane_at(px, py);
        if let Some((pane_id, rect)) = clicked {
            self.focus_pane_by_id(pane_id);
            let btn_id = pointer_btn_id(button);

            // If the hosted app captures the mouse (and Shift isn't held to bypass),
            // report the press to the PTY and skip local selection.
            if self.try_report_press(pane_id, rect, px, py, btn_id, shift, alt, ctrl) {
                return;
            }

            // Determine click count for word/line selection (Primary button only)
            let click_count = if button == xilem::masonry::core::PointerButton::Primary {
                self.update_click_state(pane_id)
            } else {
                1
            };

            if let Some(group) = self.groups.get_mut(self.active_group_index) {
                if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                    if let Some((pane, _)) = tab.pane_tree.find_mut(pane_id) {
                        let cols = pane.terminal.grid_cols();
                        let rows = pane.terminal.grid_rows();

                        let col = (((px - rect.x - PANE_MARGIN) / self.cell_w).floor().max(0.0) as usize)
                            .min(cols.saturating_sub(1));
                        let row = (((py - rect.y - PANE_MARGIN) / self.cell_h).floor().max(0.0) as usize)
                            .min(rows.saturating_sub(1));

                        if button == xilem::masonry::core::PointerButton::Primary {
                            match click_count {
                                2 => pane.terminal.start_word_selection(row as i32, col),
                                3 => pane.terminal.start_line_selection(row as i32, col),
                                _ => pane.terminal.start_selection(row as i32, col),
                            }
                            self.redraw_counter += 1;
                        } else if button == xilem::masonry::core::PointerButton::Secondary {
                            // Right click -> Paste clipboard content
                            if let Some(ref mut clip) = self.clipboard {
                                if let Ok(text) = clip.get_contents() {
                                    let _ = pane.terminal.write_input(text.as_bytes());
                                }
                            }
                            pane.terminal.clear_selection();
                            self.redraw_counter += 1;
                        }
                    }
                }
            }
        }
    }

    /// If `pane_id`'s app captures the mouse, report a press and return true.
    fn try_report_press(
        &mut self,
        pane_id: PaneId,
        rect: crate::pane_tree::Rect,
        px: f64,
        py: f64,
        btn_id: u8,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> bool {
        if shift {
            return false;
        }
        if let Some(group) = self.groups.get_mut(self.active_group_index) {
            if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                if let Some((pane, _)) = tab.pane_tree.find_mut(pane_id) {
                    if pane.terminal.mouse_is_reporting() {
                        let gx = (px - rect.x - PANE_MARGIN).max(0.0) as f32;
                        let gy = (py - rect.y - PANE_MARGIN).max(0.0) as f32;
                        pane.terminal.mouse_report(btn_id, 0, true, gx, gy, shift, alt, ctrl);
                        self.mouse_down_btn = Some(btn_id);
                        self.redraw_counter += 1;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Handle mouse move/dragging at coordinates (px, py)
    pub fn handle_mouse_move(&mut self, px: f64, py: f64, is_dragging: bool, shift: bool, alt: bool, ctrl: bool) {
        self.hover_pos = Some((px, py));
        self.redraw_counter += 1;

        if !is_dragging {
            return;
        }

        // When the app captures the mouse, report drag motion instead of selecting.
        if self.try_report_motion(px, py, shift, alt, ctrl) {
            return;
        }

        let active_id = if let Some(pane) = self.active_pane() {
            pane.id
        } else {
            return;
        };

        let active_rect = if let Some((_, r)) = self.last_pane_layouts.iter().find(|(id, _)| *id == active_id) {
            *r
        } else {
            return;
        };

        if let Some(group) = self.groups.get_mut(self.active_group_index) {
            if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                if let Some((pane, _)) = tab.pane_tree.find_mut(active_id) {
                    let cols = pane.terminal.grid_cols();
                    let rows = pane.terminal.grid_rows();

                    let col = (((px - active_rect.x - PANE_MARGIN) / self.cell_w).floor().max(0.0) as usize)
                        .min(cols.saturating_sub(1));
                    let row = (((py - active_rect.y - PANE_MARGIN) / self.cell_h).floor().max(0.0) as usize)
                        .min(rows.saturating_sub(1));

                    pane.terminal.update_selection(row as i32, col);
                    self.redraw_counter += 1;
                }
            }
        }
    }

    /// Handle mouse button release up at coordinates (px, py)
    pub fn handle_mouse_up(&mut self, px: f64, py: f64, button: xilem::masonry::core::PointerButton, shift: bool, alt: bool, ctrl: bool) {
        let btn_id = pointer_btn_id(button);
        self.mouse_down_btn = None;

        // When the app captures the mouse, report the release and skip copy-on-select.
        if self.try_report_release(px, py, btn_id, shift, alt, ctrl) {
            return;
        }

        if button == xilem::masonry::core::PointerButton::Primary {
            let active_id = if let Some(pane) = self.active_pane() {
                pane.id
            } else {
                return;
            };

            if let Some(group) = self.groups.get_mut(self.active_group_index) {
                if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                    if let Some((pane, _)) = tab.pane_tree.find_mut(active_id) {
                        if let Some(selected_text) = pane.terminal.selection_to_string() {
                            if !selected_text.is_empty() {
                                if let Some(ref mut clip) = self.clipboard {
                                    let _ = clip.set_contents(selected_text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Scroll wheel: report to the app if it captures the mouse, else scrollback.
    pub fn handle_scroll(&mut self, delta_lines: i32, px: f64, py: f64, shift: bool, alt: bool, ctrl: bool) {
        if !shift {
            if let Some(active_id) = self.active_pane().map(|p| p.id) {
                let rect = self
                    .last_pane_layouts
                    .iter()
                    .find(|(id, _)| *id == active_id)
                    .map(|(_, r)| *r);
                if let Some(rect) = rect {
                    if let Some(group) = self.groups.get_mut(self.active_group_index) {
                        if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                            if let Some((pane, _)) = tab.pane_tree.find_mut(active_id) {
                                if pane.terminal.mouse_is_reporting() {
                                    let gx = (px - rect.x - PANE_MARGIN).max(0.0) as f32;
                                    let gy = (py - rect.y - PANE_MARGIN).max(0.0) as f32;
                                    let btn = if delta_lines > 0 { 3 } else { 4 };
                                    pane.terminal.mouse_report(btn, 0, false, gx, gy, shift, alt, ctrl);
                                    self.redraw_counter += 1;
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        self.scroll_terminal(delta_lines);
    }

    /// Report a drag motion event if the active pane's app captures the mouse.
    fn try_report_motion(&mut self, px: f64, py: f64, shift: bool, alt: bool, ctrl: bool) -> bool {
        if shift {
            return false;
        }
        let btn = match self.mouse_down_btn {
            Some(b) => b,
            None => return false,
        };
        let active_id = match self.active_pane() {
            Some(p) => p.id,
            None => return false,
        };
        let rect = match self
            .last_pane_layouts
            .iter()
            .find(|(id, _)| *id == active_id)
        {
            Some((_, r)) => *r,
            None => return false,
        };
        if let Some(group) = self.groups.get_mut(self.active_group_index) {
            if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                if let Some((pane, _)) = tab.pane_tree.find_mut(active_id) {
                    if pane.terminal.mouse_is_reporting() {
                        let gx = (px - rect.x - PANE_MARGIN).max(0.0) as f32;
                        let gy = (py - rect.y - PANE_MARGIN).max(0.0) as f32;
                        pane.terminal.mouse_report(btn, 2, true, gx, gy, shift, alt, ctrl);
                        self.redraw_counter += 1;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Report a button-release event if the active pane's app captures the mouse.
    fn try_report_release(
        &mut self,
        px: f64,
        py: f64,
        btn_id: u8,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> bool {
        if shift {
            return false;
        }
        let active_id = match self.active_pane() {
            Some(p) => p.id,
            None => return false,
        };
        let rect = match self
            .last_pane_layouts
            .iter()
            .find(|(id, _)| *id == active_id)
        {
            Some((_, r)) => *r,
            None => return false,
        };
        if let Some(group) = self.groups.get_mut(self.active_group_index) {
            if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                if let Some((pane, _)) = tab.pane_tree.find_mut(active_id) {
                    if pane.terminal.mouse_is_reporting() {
                        let gx = (px - rect.x - PANE_MARGIN).max(0.0) as f32;
                        let gy = (py - rect.y - PANE_MARGIN).max(0.0) as f32;
                        pane.terminal.mouse_report(btn_id, 1, false, gx, gy, shift, alt, ctrl);
                        self.redraw_counter += 1;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Save current state to config file (font_size, window size/position)
    pub fn save_config(&self, position: Option<crate::config::WindowPosition>) {
        let mut cfg = self.user_config.clone();
        cfg.font_size = self.font_size;
        cfg.window.width = self.window_size.0.max(100.0);
        cfg.window.height = self.window_size.1.max(100.0);
        cfg.window.maximized = self.window_maximized;
        cfg.window.position = position;
        if let Ok(content) = toml::to_string_pretty(&cfg) {
            let _ = std::fs::write(crate::config::Config::path(), content);
        }
    }

    /// Focus pane directionally (left, right, up, down)
    pub fn focus_pane_directionally(&mut self, direction: &str) {
        // println!("DEBUG: focus_pane_directionally: {}", direction);
        let active_id = if let Some(pane) = self.active_pane() {
            pane.id
        } else {
            return;
        };

        // Find active pane's rect
        let active_rect = if let Some((_, r)) = self.last_pane_layouts.iter().find(|(id, _)| *id == active_id) {
            *r
        } else {
            return;
        };

        let cx_a = active_rect.x + active_rect.width / 2.0;
        let cy_a = active_rect.y + active_rect.height / 2.0;

        let mut best_pane_id = None;
        let mut best_score = f64::MAX;

        for (pane_id, rect) in &self.last_pane_layouts {
            if *pane_id == active_id {
                continue;
            }
            let cx_p = rect.x + rect.width / 2.0;
            let cy_p = rect.y + rect.height / 2.0;

            let dx = cx_p - cx_a;
            let dy = cy_p - cy_a;

            let matches = match direction {
                "left" => dx < -1.0,
                "right" => dx > 1.0,
                "up" => dy < -1.0,
                "down" => dy > 1.0,
                _ => false,
            };

            if matches {
                let score = match direction {
                    "left" => -dx + 2.0 * dy.abs(),
                    "right" => dx + 2.0 * dy.abs(),
                    "up" => -dy + 2.0 * dx.abs(),
                    "down" => dy + 2.0 * dx.abs(),
                    _ => f64::MAX,
                };
                if score < best_score {
                    best_score = score;
                    best_pane_id = Some(*pane_id);
                }
            }
        }

        if let Some(pane_id) = best_pane_id {
            // Activate the new pane
            if let Some(group) = self.groups.get_mut(self.active_group_index) {
                if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                    if tab.active_pane_id != pane_id {
                        // Deactivate old active pane
                        if let Some((old_pane, _)) = tab.pane_tree.find_mut(tab.active_pane_id) {
                            old_pane.set_active(false);
                        }
                        // Activate clicked pane
                        if let Some((new_pane, _)) = tab.pane_tree.find_mut(pane_id) {
                            new_pane.set_active(true);
                            tab.active_pane_id = pane_id;
                            self.status_message = format!("Focused pane {}", pane_id);
                            self.redraw_counter += 1;
                        }
                    }
                }
            }
        }
    }
}

/// Map a masonry pointer button to the adapter's btn_id (0=left,1=right,2=middle,255=other).
fn pointer_btn_id(button: xilem::masonry::core::PointerButton) -> u8 {
    use xilem::masonry::core::PointerButton::*;
    match button {
        Primary => 0,
        Secondary => 1,
        Auxiliary => 2,
        _ => 255,
    }
}

