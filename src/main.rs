// GUI subsystem: no console window when launched by double-click.
// Tracing output still honors stderr redirection (files) from a shell.
#![windows_subsystem = "windows"]

mod app;
mod config;
mod group;
mod keyboard;
mod pane;
mod pane_tree;
mod single_instance;
mod tab;
mod terminal;
mod terminal_keyboard;
mod terminal_render;
mod terminal_view;

#[cfg(target_os = "windows")]
pub mod win_util {
    use std::ffi::c_void;

    extern "system" {
        fn GetActiveWindow() -> *mut c_void;
        fn ReleaseCapture() -> i32;
        fn SendMessageA(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn ShowWindow(hwnd: *mut c_void, cmd_show: i32) -> i32;
        fn IsZoomed(hwnd: *mut c_void) -> i32;
        fn GetWindowRect(hwnd: *mut c_void, rect: *mut std::ffi::c_void) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(hwnd: *mut c_void, attr: u32, val: *const u32, len: u32) -> i32;
    }

    /// Windows 11: request rounded window corners from DWM.
    pub fn set_rounded_corners() {
        const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
        const DWMWCP_ROUND: u32 = 2;
        unsafe {
            let hwnd = GetActiveWindow();
            if !hwnd.is_null() {
                let pref = DWMWCP_ROUND;
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &pref,
                    std::mem::size_of::<u32>() as u32,
                );
            }
        }
    }

    const WM_NCLBUTTONDOWN: u32 = 0x00A1;
    const HTCAPTION: usize = 2;
    const SW_MINIMIZE: i32 = 6;
    const SW_MAXIMIZE: i32 = 3;
    const SW_RESTORE: i32 = 9;

    pub fn drag_active_window() {
        unsafe {
            let hwnd = GetActiveWindow();
            if !hwnd.is_null() {
                ReleaseCapture();
                SendMessageA(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0);
            }
        }
    }

    pub fn minimize_active_window() {
        unsafe {
            let hwnd = GetActiveWindow();
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
        }
    }

    pub fn maximize_active_window() {
        unsafe {
            let hwnd = GetActiveWindow();
            if !hwnd.is_null() {
                if IsZoomed(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                } else {
                    ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        }
    }

    pub fn close_active_window() {
        std::process::exit(0);
    }

    extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> *mut c_void;
        fn SetForegroundWindow(hwnd: *mut c_void) -> i32;
        fn IsIconic(hwnd: *mut c_void) -> i32;
    }

    /// Restore and focus the main window (used by single-instance activation).
    pub fn bring_to_front() {
        const SW_RESTORE: i32 = 9;
        let title: Vec<u16> = "amux - Terminal Multiplexer"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd.is_null() {
                return;
            }
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }

    #[repr(C)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    pub fn get_window_position() -> Option<crate::config::WindowPosition> {
        unsafe {
            let hwnd = GetActiveWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if GetWindowRect(hwnd, &mut rect as *mut _ as *mut std::ffi::c_void) != 0 {
                Some(crate::config::WindowPosition {
                    x: rect.left as f64,
                    y: rect.top as f64,
                })
            } else {
                None
            }
        }
    }

    pub fn is_window_maximized() -> bool {
        unsafe {
            let hwnd = GetActiveWindow();
            if hwnd.is_null() {
                return false;
            }
            IsZoomed(hwnd) != 0
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod win_util {
    pub fn drag_active_window() {}
    pub fn minimize_active_window() {}
    pub fn maximize_active_window() {}
    pub fn close_active_window() {
        std::process::exit(0);
    }
    pub fn get_window_position() -> Option<crate::config::WindowPosition> {
        None
    }
    pub fn is_window_maximized() -> bool {
        false
    }
    pub fn bring_to_front() {}
}

use app::App;
use xilem::core::fork;
use xilem::view::{flex_col, label, flex_row, sized_box, zstack, task_raw, FlexExt, CrossAxisAlignment};
use xilem::style::Style;
use xilem::masonry::layout::Length;
use xilem::masonry::properties::Gap;
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};
use xilem::dpi::LogicalSize;
use xilem::masonry::peniko::Color;
use std::time::Duration;

// ─── Catppuccin Mocha Colors ───
const CRUST: Color = Color::from_rgba8(0x11, 0x11, 0x1b, 0xff);
const BASE: Color = Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff);
const MANTLE: Color = Color::from_rgba8(0x18, 0x18, 0x25, 0xff);
const SURFACE0: Color = Color::from_rgba8(0x31, 0x32, 0x44, 0xff);
const SURFACE1: Color = Color::from_rgba8(0x45, 0x47, 0x5a, 0xff);
const TEXT_COLOR: Color = Color::from_rgba8(0xc6, 0xd0, 0xf5, 0xff);
const BLUE: Color = Color::from_rgba8(0x89, 0xb4, 0xfa, 0xff);
#[allow(dead_code)]
const TEAL: Color = Color::from_rgba8(0x94, 0xe2, 0xd5, 0xff);

fn sidebar_item(idx: usize, text: String, is_active: bool, is_plus: bool) -> impl WidgetView<App> + use<> {
    let bg = if is_active {
        BASE
    } else {
        Color::from_rgba8(0x00, 0x00, 0x00, 0x00) // transparent
    };
    let text_color = if is_active { BLUE } else { SURFACE1 };
    let l = label(text).text_size(14.0).color(text_color);
    sized_box(
        crate::terminal_keyboard::focusless_button(l, move |data: &mut App| {
            if is_plus {
                data.add_group();
            } else {
                data.switch_group(idx);
            }
        })
    )
    .fixed_width(Length::px(32.0))
    .fixed_height(Length::px(32.0))
    .background(bg)
    .padding(2.0)
}

/// Left sidebar - vertical group toolbar (VSCode-style activity bar)
fn sidebar(state: &App) -> impl WidgetView<App> + use<> {
    let mut items: Vec<_> = if state.show_sidebar {
        state.groups.iter().enumerate().map(|(idx, group)| {
            let is_active = idx == state.active_group_index;
            sidebar_item(idx, group.icon.clone(), is_active, false)
        }).collect()
    } else {
        Vec::new()
    };

    if state.show_sidebar {
        // Group Add button
        items.push(sidebar_item(999, "➕".to_string(), false, true));
    }

    let width = if state.show_sidebar { 40.0 } else { 0.0 };

    sized_box(
        flex_col((
            flex_col(items)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .flex(1.0),
        ))
    )
    .fixed_width(Length::px(width))
    .background(if state.show_sidebar { CRUST } else { Color::from_rgba8(0, 0, 0, 0) })
    .padding(if state.show_sidebar { 2.0 } else { 0.0 })
}

/// Right sidebar - system monitor panel (toggled via Alt+Ctrl+B)
fn right_sidebar(state: &App) -> impl WidgetView<App> + use<> {
    let width = if state.show_right_sidebar { 120.0 } else { 0.0 };
    let text_color = if state.show_right_sidebar { TEXT_COLOR } else { Color::from_rgba8(0, 0, 0, 0) };
    let blue_color = if state.show_right_sidebar { BLUE } else { Color::from_rgba8(0, 0, 0, 0) };
    let surface1_color = if state.show_right_sidebar { SURFACE1 } else { Color::from_rgba8(0, 0, 0, 0) };
    let spacer_height = if state.show_right_sidebar { 10.0 } else { 0.0 };

    let content = flex_col((
        label("Right Sidebar").weight(xilem::masonry::parley::style::FontWeight::BOLD).text_size(12.0).color(blue_color),
        sized_box(label("")).fixed_height(Length::px(spacer_height)),
        label("System Monitor").text_size(10.0).color(surface1_color),
        label("CPU: 12%").text_size(11.0).color(text_color),
        label("MEM: 42%").text_size(11.0).color(text_color),
    ))
    .cross_axis_alignment(CrossAxisAlignment::Start);

    sized_box(content)
        .fixed_width(Length::px(width))
        .background(if state.show_right_sidebar { CRUST } else { Color::from_rgba8(0, 0, 0, 0) })
        .padding(if state.show_right_sidebar { 10.0 } else { 0.0 })
}

fn tab_item(idx: usize, text: String, is_active: bool) -> impl WidgetView<App> + use<> {
    let bg_color = if is_active {
        SURFACE0
    } else {
        Color::from_rgba8(0x1e, 0x1e, 0x2e, 0x00) // Transparent
    };
    let fg = if is_active {
        TEXT_COLOR
    } else {
        Color::from_rgba8(0x7f, 0x84, 0x9c, 0xff) // muted (subtext0)
    };

    // Rounded index-card tab: pill-shaped card, active one filled.
    flex_row((
        crate::terminal_keyboard::focusless_button(label(text).text_size(12.0).color(fg), move |data: &mut App| {
            data.switch_tab(idx);
        })
        .padding(0.0),
        crate::terminal_keyboard::focusless_button(label("×").text_size(13.0).color(fg), move |data: &mut App| {
            data.remove_tab_at(idx);
        })
        .padding(0.0),
    ))
    .background(bg_color)
    .corner_radius(8.0)
    .padding(xilem::masonry::properties::Padding { left: 10.0, right: 6.0, top: 4.0, bottom: 4.0 })
}

/// Tab bar with active group's tabs
fn tab_bar(state: &App) -> impl WidgetView<App> + use<> {
    let mut items = Vec::new();
    if let Some(group) = state.groups.get(state.active_group_index) {
        for (idx, tab) in group.tabs.iter().enumerate() {
            let active_pane_id = tab.active_pane_id;
            let pane_title = if let Some((pane, _)) = tab.pane_tree.find(active_pane_id) {
                pane.terminal.title().unwrap_or_else(|| "Terminal".to_string())
            } else {
                "Terminal".to_string()
            };

            let displayed_title = if pane_title.chars().count() > 20 {
                let truncated: String = pane_title.chars().take(17).collect();
                format!("{}...", truncated)
            } else {
                pane_title
            };

            let pane_count = tab.pane_tree.pane_count();
            let label_text = if pane_count > 1 {
                format!(" {} [{}] ", displayed_title, pane_count)
            } else {
                format!(" {} ", displayed_title)
            };
            let is_active = idx == group.active_tab_index;
            items.push(tab_item(idx, label_text, is_active));
        }
    }

    let add_tab_btn = crate::terminal_keyboard::focusless_button(label("➕"), |data: &mut App| {
        data.add_tab();
    });

    let prefix_indicator = if state.prefix_mode {
        label(" [PREFIX] ").weight(xilem::masonry::parley::style::FontWeight::BOLD)
    } else {
        label("")
    };

    flex_row((
        flex_row(items),
        add_tab_btn,
        prefix_indicator,
    ))
    .background(MANTLE)
    .padding(1.0)
}



/// Main application layout
fn app_logic(data: &mut App) -> impl WidgetView<App> + use<> {
    // Redraw trigger task to handle event-driven redraws
    let _redraw_trigger = task_raw(
        |proxy, state: &mut App| {
            state.register_redraw_proxy(proxy.clone());
            #[cfg(target_os = "windows")]
            win_util::set_rounded_corners();
            async move {
                // println!("DEBUG: task_raw future spawned (now 100% event-driven)!");
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            }
        },
        |state: &mut App, ()| {
            // println!("DEBUG: task_raw event callback received message!");
            if state.check_and_clear_redraw() {
                state.redraw_counter += 1;
                // println!("DEBUG: task_raw check_and_clear_redraw was TRUE! redraw_counter updated");
            }
            state.poll_single_instance();
            state.reap_exited_panes();
            state.apply_pending_resizes();
            if !state.running {
                // masonry_winit only checks its exit flag after window events,
                // but this callback runs on the user-event path, so the flag
                // would never be acted on. Save config and exit directly.
                state.window_maximized = win_util::is_window_maximized();
                let pos = win_util::get_window_position();
                state.save_config(pos);
                state.shutdown_single();
                std::process::exit(0);
            }
        },
    );

    let terminal_area = terminal_view::terminal_canvas_view(data.redraw_counter);
    let keyboard_capture = crate::terminal_keyboard::terminal_keyboard_view(|state: &mut App, action: crate::terminal_keyboard::TerminalAction| {
        match action {
            crate::terminal_keyboard::TerminalAction::Key(key_action) => {
                // println!("DEBUG: Key received: key='{}', ctrl={}, alt={}, shift={}", key_action.key, key_action.ctrl, key_action.alt, key_action.shift);
                if !key_action.key.is_empty() {
                    state.handle_raw_key_event(&key_action.key, key_action.ctrl, key_action.alt, key_action.shift);
                    true
                } else {
                    false
                }
            }
            crate::terminal_keyboard::TerminalAction::Scroll { delta_lines, x, y, shift, alt, ctrl } => {
                state.handle_scroll(delta_lines, x, y, shift, alt, ctrl);
                true
            }
            crate::terminal_keyboard::TerminalAction::MouseDown { x, y, button, shift, alt, ctrl } => {
                state.handle_mouse_down(x, y, button, shift, alt, ctrl);
                true
            }
            crate::terminal_keyboard::TerminalAction::MouseMove { x, y, is_dragging, shift, alt, ctrl } => {
                state.handle_mouse_move(x, y, is_dragging, shift, alt, ctrl);
                true
            }
            crate::terminal_keyboard::TerminalAction::MouseUp { x, y, button, shift, alt, ctrl } => {
                state.handle_mouse_up(x, y, button, shift, alt, ctrl);
                true
            }
            crate::terminal_keyboard::TerminalAction::TextCommit(text) => {
                if let Some(pane) = state.active_pane_mut() {
                    let _ = pane.terminal.write_input(text.as_bytes());
                }
                true
            }
        }
    });

    // Main content area (right side)
    let content = flex_col((
        tab_bar(data),
        zstack((
            terminal_area,
            keyboard_capture,
        )).flex(1.0),
    )).cross_axis_alignment(CrossAxisAlignment::Stretch).prop(Gap::ZERO);

    // Root layout: sidebar | content | right_sidebar
    let main_view = flex_row((
        sidebar(data),
        content.flex(1.0),
        right_sidebar(data),
    )).cross_axis_alignment(CrossAxisAlignment::Stretch).prop(Gap::ZERO);

    fork(main_view, Some(_redraw_trigger))
}

fn main() {
    // Silence verbose framework debug logs by defaulting to "info" level
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,icu_provider=error")),
        )
        .try_init();

    let cfg = config::Config::load();
    log::info!("Loaded config from {:?}", config::Config::path());
    log::info!("  font_size: {}, window: {}x{}", cfg.font_size, cfg.window.width, cfg.window.height);

    // CLI: `amux <path>` starts shells in the given directory.
    let mut working_dir: Option<std::path::PathBuf> = None;
    let mut raw_path: Option<std::ffi::OsString> = None;
    {
        use std::ffi::OsString;
        let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
        if let Some(first) = raw.first() {
            let shown = first.to_string_lossy().into_owned();
            if shown == "-h" || shown == "--help" {
                eprintln!(
                    "Usage: amux [path]\n  \
                     path   directory to open the terminal in\n  \
                     With single_instance=true in config.toml, a second `amux`\n  \
                     forwards its path to the running window as a new tab and exits."
                );
                return;
            }
            let path = std::path::PathBuf::from(first);
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir().map(|c| c.join(&path)).unwrap_or(path.clone())
            };
            if !abs.is_dir() {
                eprintln!("amux: not a directory: {shown}");
                std::process::exit(1);
            }
            working_dir = Some(abs);
            raw_path = Some(first.clone());
        }
    }

    // Single-instance mode: first instance serves IPC; later ones forward and exit.
    let mut single_server = None;
    if cfg.single_instance {
        if let Some(srv) = single_instance::start_first_instance() {
            single_server = Some(srv);
        } else if single_instance::forward_to_running(raw_path.as_deref()) {
            return;
        }
        // Forwarding failed (stale discovery): fall through and open our own window.
    }

    let mut app_state = App::new(cfg.clone(), working_dir);
    if let Some(srv) = single_server {
        app_state.set_single_server(srv);
    }

    let mut win_opts = WindowOptions::new("amux - Terminal Multiplexer")
        .with_initial_inner_size(LogicalSize::new(cfg.window.width, cfg.window.height))
        .with_decorations(true)
        .on_close(|state: &mut App| {
            state.window_maximized = win_util::is_window_maximized();
            let pos = win_util::get_window_position();
            state.save_config(pos);
        });

    if let Some(pos) = &cfg.window.position {
        use xilem::dpi::LogicalPosition;
        win_opts = win_opts.with_initial_position(LogicalPosition::new(pos.x, pos.y));
    }

    const ICON_RGBA32: &[u8] = include_bytes!("../assets/icon_rgba32.bin");
    if let Ok(icon) = xilem::winit::window::Icon::from_rgba(ICON_RGBA32.to_vec(), 32, 32) {
        win_opts = win_opts.with_initial_window_icon(Some(icon));
    }

    let app = Xilem::new_simple(app_state, app_logic, win_opts);

    app.run_in(EventLoop::with_user_event()).unwrap();
}
