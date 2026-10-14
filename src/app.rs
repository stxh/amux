use crate::config::Config;
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
    /// Global ID counter for groups
    pub next_group_id: usize,
    /// Terminal configuration
    pub terminal_config: TerminalConfig,
    /// Whether we're in prefix mode (waiting for a command after Ctrl+B)
    pub prefix_mode: bool,
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
    /// Single-instance IPC server (first instance only)
    pub single_server: Option<std::sync::Arc<crate::single_instance::ServerState>>,
    /// Whether the system settings overlay is open
    pub show_settings: bool,
    /// Hit rects of the settings overlay controls (rebuilt on every frame)
    pub settings_items: Vec<(crate::terminal_render::SettingsAction, crate::pane_tree::Rect)>,
    /// Bounds of the settings overlay panel (None while closed)
    pub settings_panel: Option<crate::pane_tree::Rect>,
    /// Scrollbars of every pane for the current frame (hit testing)
    pub scrollbars: Vec<crate::terminal_render::ScrollbarGeometry>,
    /// Active scrollbar thumb drag: pane plus pointer-to-thumb-centre offset
    scrollbar_drag: Option<ScrollbarDrag>,
    /// Edge auto-scroll while a selection drag holds the pointer outside a pane
    auto_scroll: Option<AutoScroll>,
    /// Mirrors `auto_scroll.is_some()` so the UI timer only wakes the loop
    /// when scrolling is actually needed.
    pub auto_scroll_active: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// In-flight scrollbar drag.
///
/// The geometry is snapshotted at grab time: the engine's `scrollbar()` call is
/// expensive for pinned viewports, so re-reading it on every mouse move would
/// make the drag stutter.
#[derive(Clone, Copy)]
struct ScrollbarDrag {
    pane_id: PaneId,
    /// Pointer y (client space) when the drag started.
    start_y: f64,
    /// Thumb travel available, in pixels.
    movable: f64,
    /// Scrollable rows above the viewport.
    max_scroll: f64,
    /// Engine offset when the drag started (0 = oldest row).
    base_offset: f64,
    /// Last target handed to the engine; keeps sub-row steps from being lost.
    applied: f64,
}

/// Edge auto-scroll driven by a selection drag past a pane's row bounds.
#[derive(Clone, Copy)]
struct AutoScroll {
    pane_id: PaneId,
    /// Rows per tick; positive scrolls into history, like `scroll_viewport`.
    step: i32,
}

impl AppState for App {
    fn keep_running(&self) -> bool {
        self.running
    }
}

impl App {
    pub fn new(cfg: Config, working_dir: Option<std::path::PathBuf>) -> Self {
        let mut config = TerminalConfig::default();
        config.working_dir = working_dir;

        // Create first group with one tab and one pane
        let group = TerminalGroup::new(1, 1, &config);

        Self {
            groups: vec![group],
            active_group_index: 0,
            next_pane_id: 2,   // pane 1 already used
            next_group_id: 2,  // group 1 already used
            terminal_config: config,
            prefix_mode: false,
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
            single_server: None,
            show_settings: false,
            settings_items: Vec::new(),
            settings_panel: None,
            scrollbars: Vec::new(),
            scrollbar_drag: None,
            auto_scroll: None,
            auto_scroll_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn set_single_server(&mut self, server: std::sync::Arc<crate::single_instance::ServerState>) {
        if let Some(proxy) = &self.redraw_proxy {
            server.set_proxy(proxy.clone());
        }
        self.single_server = Some(server);
    }

    /// Wake the event loop so a redraw (or an exit request) is handled promptly.
    pub fn wake(&self) {
        if let Some(proxy) = &self.redraw_proxy {
            let _ = proxy.message(());
        }
    }

    // ─── System settings overlay ───

    /// Open or close the settings overlay (the gear button, Ctrl+, Esc).
    pub fn toggle_settings(&mut self) {
        self.show_settings = !self.show_settings;
        self.status_message = if self.show_settings {
            "Settings".to_string()
        } else {
            "Settings closed".to_string()
        };
        self.redraw_counter += 1;
        self.wake();
    }

    /// Close the settings overlay if it is open.
    pub fn close_settings(&mut self) {
        if self.show_settings {
            self.show_settings = false;
            self.redraw_counter += 1;
            self.wake();
        }
    }

    /// Apply a control of the settings overlay.
    pub fn apply_settings_action(&mut self, action: crate::terminal_render::SettingsAction) {
        use crate::terminal_render::SettingsAction;
        match action {
            SettingsAction::FontMinus => {
                self.font_size = (self.font_size - 1.0).max(6.0);
                self.status_message = format!("Font size: {}", self.font_size.round() as i64);
            }
            SettingsAction::FontPlus => {
                self.font_size = (self.font_size + 1.0).min(72.0);
                self.status_message = format!("Font size: {}", self.font_size.round() as i64);
            }
            SettingsAction::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                self.status_message = format!("Sidebar visibility: {}", self.show_sidebar);
            }
            SettingsAction::ToggleRightSidebar => {
                self.show_right_sidebar = !self.show_right_sidebar;
                self.status_message = format!("Right sidebar visibility: {}", self.show_right_sidebar);
            }
            SettingsAction::ToggleSingleInstance => {
                self.user_config.single_instance = !self.user_config.single_instance;
                self.status_message = format!("Single instance: {}", self.user_config.single_instance);
            }
            SettingsAction::OpenConfigDir => {
                crate::config::open_config_dir();
                self.status_message = "Opened config directory".to_string();
            }
            SettingsAction::Close => {
                self.close_settings();
                return;
            }
            SettingsAction::Quit => {
                self.running = false;
                self.status_message = "Quitting".to_string();
                self.redraw_counter += 1;
                self.wake();
                return;
            }
        }
        self.redraw_counter += 1;
        self.wake();
    }

    /// Handle queued single-instance requests: open tabs and raise the window.
    pub fn poll_single_instance(&mut self) {
        let Some(server) = self.single_server.clone() else {
            return;
        };
        let (paths, activate) = server.take_requests();
        let any_paths = !paths.is_empty();
        for path in paths {
            let pb = std::path::PathBuf::from(&path);
            if !pb.is_dir() {
                self.status_message = format!("Not a directory: {}", path);
                continue;
            }
            self.add_tab_with_cwd(Some(pb));
            self.status_message = format!("Opened tab at {}", path);
        }
        if activate || any_paths {
            crate::win_util::bring_to_front();
        }
    }

    pub fn shutdown_single(&self) {
        if let Some(server) = &self.single_server {
            server.shutdown();
        }
    }

    /// Register the Xilem message proxy to wake up the event loop when terminal stdout changes
    pub fn register_redraw_proxy(&mut self, proxy: xilem::core::MessageProxy<()>) {
        // println!("DEBUG: Registering redraw_proxy on App!");
        self.redraw_proxy = Some(proxy.clone());
        if let Some(server) = &self.single_server {
            server.set_proxy(proxy.clone());
        }
        
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
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let mut group = TerminalGroup::new(group_id, pane_id, &self.terminal_config);
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
        let config = self.terminal_config.clone();
        self.add_tab_with_config(config);
    }

    /// Add a new tab whose shells start in `cwd` (single-instance `amux <path>`).
    pub fn add_tab_with_cwd(&mut self, cwd: Option<std::path::PathBuf>) {
        let mut config = self.terminal_config.clone();
        config.working_dir = cwd;
        self.add_tab_with_config(config);
    }

    fn add_tab_with_config(&mut self, config: crate::terminal::TerminalConfig) {
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        let proxy = self.redraw_proxy.clone(); // Clone first to avoid borrow conflicts
        if let Some(group) = self.active_group_mut() {
            group.add_tab(pane_id, &config);
            if let Some(p) = proxy {
                if let Some(tab) = group.tabs.last_mut() {
                    tab.pane_tree.for_each_mut(&mut |pane, _| {
                        *pane.terminal.redraw_notifier.lock().unwrap() = Some(p.clone());
                    });
                }
            }
            let tab_count = self.groups[self.active_group_index].tab_count();
            self.status_message = format!("New tab ({} tabs)", tab_count);
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
        let mut aborted: Vec<(usize, usize, PaneId, u32)> = Vec::new();
        for (gi, group) in self.groups.iter().enumerate() {
            for (ti, tab) in group.tabs.iter().enumerate() {
                tab.pane_tree.for_each(&mut |pane, _| {
                    if pane.terminal.shell_exited() {
                        exited.push((gi, ti, pane.id));
                    } else if let Some(code) = pane.terminal.take_shell_abort() {
                        aborted.push((gi, ti, pane.id, code));
                    }
                });
            }
        }
        if aborted.is_empty() && exited.is_empty() {
            return;
        }
        // Abnormal shell exit (e.g. killed by a child TUI such as opencode's
        // parent-kill bug): keep the pane, print a notice instead of closing.
        for (gi, ti, pane_id, code) in aborted {
            if let Some((pane, _)) = self.groups[gi].tabs[ti].pane_tree.find(pane_id) {
                pane.terminal.print_notice(&format!(
                    "[amux] shell terminated abnormally (code {code}); pane stays open, close with Ctrl+B x"));
            }
            self.status_message = format!("Shell exited abnormally (code {code})");
            self.redraw_counter += 1;
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
        // 0. System settings overlay is modal: it swallows every click.
        if self.show_settings {
            let hit = self
                .settings_items
                .iter()
                .find(|(_, rect)| {
                    px >= rect.x && px <= rect.x + rect.width && py >= rect.y && py <= rect.y + rect.height
                })
                .map(|(action, _)| *action);
            match hit {
                Some(action) => self.apply_settings_action(action),
                // Any click outside a control dismisses the overlay.
                None => self.close_settings(),
            }
            return;
        }

        // 1. Scrollbar: pressing anywhere on the track takes the pointer and
        // starts a thumb drag, so it must win over selection, mouse reporting
        // and focus. Pressing outside the thumb additionally pages the view.
        if let Some(scrollbar) = self.scrollbars.iter().copied().find(|sb| sb.contains(px, py)) {
            self.focus_pane_by_id(scrollbar.pane_id);
            let base_offset = self.pane_scroll_offset(scrollbar.pane_id);
            self.scrollbar_drag = Some(ScrollbarDrag {
                pane_id: scrollbar.pane_id,
                start_y: py,
                movable: (scrollbar.track_h - scrollbar.thumb_h).max(1.0),
                max_scroll: scrollbar.max_scroll,
                base_offset,
                applied: base_offset,
            });
            // Capture the pointer at the OS level so the drag keeps tracking
            // after the cursor leaves the window.
            crate::win_util::capture_mouse();
            if !scrollbar.thumb_contains(py) {
                // Track press: page the viewport, towards history when the click
                // was above the thumb and towards the active area when below.
                let page = self.pane_viewport_rows(scrollbar.pane_id).saturating_sub(1).max(1) as i32;
                let delta = if py < scrollbar.thumb_y { page } else { -page };
                self.scroll_pane_by_id(scrollbar.pane_id, delta);
                // Keep the drag baseline in step with the page we just applied.
                if let Some(drag) = self.scrollbar_drag.as_mut() {
                    let moved = (drag.base_offset + f64::from(delta)).clamp(0.0, drag.max_scroll);
                    drag.base_offset = moved;
                    drag.applied = moved;
                }
            }
            self.redraw_counter += 1;
            return;
        }
        self.scrollbar_drag = None;

        // 2. Check if clicked a pane's top-right floating close button (if split panes are present)
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
            // Capture the mouse for any primary press so a drag keeps receiving
            // moves — and its release — after the cursor leaves the window.
            if button == xilem::masonry::core::PointerButton::Primary {
                crate::win_util::capture_mouse();
            }
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

                        let col = (((px - rect.x) / self.cell_w).floor().max(0.0) as usize)
                            .min(cols.saturating_sub(1));
                        let row = (((py - rect.y) / self.cell_h).floor().max(0.0) as usize)
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
                        let gx = (px - rect.x).max(0.0) as f32;
                        let gy = (py - rect.y).max(0.0) as f32;
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

    // ─── Scrollbar dragging ───

    /// Move the dragged thumb to follow the pointer and scroll its pane.
    ///
    /// Everything is derived from the snapshot taken at grab time, so no engine
    /// query happens per mouse move. Sub-row steps are carried in `applied`
    /// instead of being dropped, which keeps slow drags continuous.
    fn update_scrollbar_drag(&mut self, drag: ScrollbarDrag, py: f64) {
        if drag.max_scroll <= 0.0 {
            return;
        }
        let travelled = (py - drag.start_y) / drag.movable * drag.max_scroll;
        // Ghostty's scrollbar offset counts from the top of the scrollback, so
        // the tail of the track is the live end. `scroll_viewport` takes a
        // positive delta for scrolling up into history, i.e. for decreasing
        // the offset, so the step is the negative of the remaining distance.
        let target = (drag.base_offset + travelled).clamp(0.0, drag.max_scroll);
        let step = (drag.applied - target).round() as i32;
        if step == 0 {
            return;
        }
        self.scroll_pane_by_id(drag.pane_id, step);
        if let Some(drag) = self.scrollbar_drag.as_mut() {
            drag.applied -= f64::from(step);
        }
        self.redraw_counter += 1;
    }

    /// Drop every in-flight drag (thumb, selection edge scroll), e.g. when the
    /// window loses the pointer.
    pub fn cancel_drag(&mut self) {
        self.scrollbar_drag = None;
        self.set_auto_scroll(None);
        crate::win_util::release_mouse();
        self.redraw_counter += 1;
        self.wake();
    }

    // ─── Edge auto-scroll (selection dragged outside a pane) ───

    /// Arm or disarm edge auto-scroll from a selection drag position.
    ///
    /// Triggered only past the pane's first/last row, so the scrollback scrolls
    /// while the user keeps the button down outside it.
    fn update_edge_auto_scroll(&mut self, pane_id: PaneId, rect: &crate::pane_tree::Rect, py: f64) {
        const EDGE_MARGIN: f64 = 6.0;
        let overflow_up = (rect.y + EDGE_MARGIN - py).max(0.0);
        let overflow_down = (py - (rect.y + rect.height - EDGE_MARGIN)).max(0.0);

        let armed = if overflow_up > 0.0 {
            Some((overflow_up, 1))
        } else if overflow_down > 0.0 {
            Some((overflow_down, -1))
        } else {
            None
        };

        match armed {
            // Ramp with distance: a row per tick at the edge, up to ten.
            Some((overflow, sign)) => {
                let step = ((1.0 + overflow / 30.0).floor() as i32).clamp(1, 10) * sign;
                self.set_auto_scroll(Some(AutoScroll { pane_id, step }));
            }
            None => self.set_auto_scroll(None),
        }
    }

    /// Set the auto-scroll state, keeping the timer's flag in sync.
    fn set_auto_scroll(&mut self, auto: Option<AutoScroll>) {
        let changed = self.auto_scroll.is_some() != auto.is_some();
        self.auto_scroll = auto;
        self.auto_scroll_active
            .store(auto.is_some(), std::sync::atomic::Ordering::Relaxed);
        if changed {
            self.redraw_counter += 1;
        }
    }

    /// One timer tick: scroll the pane by one auto-scroll step.
    ///
    /// The engine clamps at the ends of the scrollback, and the selection is
    /// content-anchored, so scrolling extends what is selected.
    pub fn apply_auto_scroll(&mut self) {
        let Some(auto) = self.auto_scroll else {
            return;
        };
        self.scroll_pane_by_id(auto.pane_id, auto.step);
        self.redraw_counter += 1;
    }

    /// Scroll a specific pane by `delta_lines` (positive = up into history).
    fn scroll_pane_by_id(&mut self, pane_id: PaneId, delta_lines: i32) {
        for group in &mut self.groups {
            for tab in &mut group.tabs {
                if let Some((pane, _)) = tab.pane_tree.find_mut(pane_id) {
                    pane.terminal.scroll_viewport(delta_lines);
                    self.redraw_counter += 1;
                    return;
                }
            }
        }
    }

    /// Visible rows of a pane's viewport (used as the track-click page size).
    fn pane_viewport_rows(&self, pane_id: PaneId) -> usize {
        self.find_pane(pane_id).map_or(0, |pane| pane.terminal.grid_rows())
    }

    /// Current scroll offset of a pane, in rows from the top of the scrollback.
    fn pane_scroll_offset(&self, pane_id: PaneId) -> f64 {
        self.find_pane(pane_id).map_or(0.0, |pane| pane.terminal.scrollbar_state().1 as f64)
    }

    /// Look up a pane by id across all groups and tabs.
    fn find_pane(&self, pane_id: PaneId) -> Option<&Pane> {
        self.groups
            .iter()
            .flat_map(|group| group.tabs.iter())
            .find_map(|tab| tab.pane_tree.find(pane_id).map(|(pane, _)| pane))
    }

    /// Handle mouse move/dragging at coordinates (px, py)
    pub fn handle_mouse_move(&mut self, px: f64, py: f64, is_dragging: bool, shift: bool, alt: bool, ctrl: bool) {
        // Only repaint when the hover position actually moved: a move event that
        // lands on the same cell must not re-record the scene.
        if self.hover_pos != Some((px, py)) {
            self.hover_pos = Some((px, py));
            self.redraw_counter += 1;
        }

        // Scrollbar drag takes precedence over the widget's own drag state: it
        // is driven by `scrollbar_drag`, which outlives the press that set it.
        if let Some(drag) = self.scrollbar_drag {
            self.update_scrollbar_drag(drag, py);
            return;
        }
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
            for tab in &mut group.tabs {
                if let Some((pane, _)) = tab.pane_tree.find_mut(active_id) {
                    let cols = pane.terminal.grid_cols();
                    let rows = pane.terminal.grid_rows();

                    let col = (((px - active_rect.x) / self.cell_w).floor().max(0.0) as usize)
                        .min(cols.saturating_sub(1));
                    let row = (((py - active_rect.y) / self.cell_h).floor().max(0.0) as usize)
                        .min(rows.saturating_sub(1));

                    pane.terminal.update_selection(row as i32, col);
                    self.redraw_counter += 1;
                    break;
                }
            }
        }

        // Dragging the selection past a pane's rows keeps the view scrolling.
        self.update_edge_auto_scroll(active_id, &active_rect, py);
    }

    /// Handle mouse button release up at coordinates (px, py)
    pub fn handle_mouse_up(&mut self, px: f64, py: f64, button: xilem::masonry::core::PointerButton, shift: bool, alt: bool, ctrl: bool) {
        let btn_id = pointer_btn_id(button);
        self.mouse_down_btn = None;
        // Releasing the button ends any drag and edge auto-scroll, wherever the
        // pointer is.
        self.scrollbar_drag = None;
        self.set_auto_scroll(None);
        crate::win_util::release_mouse();

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
                                    let gx = (px - rect.x).max(0.0) as f32;
                                    let gy = (py - rect.y).max(0.0) as f32;
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
                        let gx = (px - rect.x).max(0.0) as f32;
                        let gy = (py - rect.y).max(0.0) as f32;
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
                        let gx = (px - rect.x).max(0.0) as f32;
                        let gy = (py - rect.y).max(0.0) as f32;
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

