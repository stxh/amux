mod app;
mod group;
mod keyboard;
mod pane;
mod pane_tree;
mod tab;
mod terminal;
mod terminal_keyboard;
mod terminal_render;
mod terminal_view;

use app::App;
use xilem::core::fork;
use xilem::view::{flex_col, label, flex_row, sized_box, zstack, task_raw, FlexExt, CrossAxisAlignment};
use xilem::style::Style;
use xilem::masonry::layout::Length;
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
const TEAL: Color = Color::from_rgba8(0x94, 0xe2, 0xd5, 0xff);

fn sidebar_item(idx: usize, text: String, is_active: bool, is_plus: bool) -> impl WidgetView<App> + use<> {
    let bg = if is_active {
        SURFACE0
    } else {
        Color::from_rgba8(0x00, 0x00, 0x00, 0x00) // transparent
    };
    let l = if is_active {
        label(text).weight(xilem::masonry::parley::style::FontWeight::BOLD)
    } else {
        label(text)
    };
    sized_box(
        crate::terminal_keyboard::focusless_button(l, move |data: &mut App| {
            if is_plus {
                data.add_group();
            } else {
                data.switch_group(idx);
            }
        })
    )
    .background(bg)
    .padding(4.0)
}

/// Left sidebar - vertical group toolbar (compact, icon-based)
fn sidebar(state: &App) -> impl WidgetView<App> + use<> {
    let mut items: Vec<_> = state.groups.iter().enumerate().map(|(idx, group)| {
        let is_active = idx == state.active_group_index;
        sidebar_item(idx, group.icon.clone(), is_active, false)
    }).collect();

    // Group Add button
    items.push(sidebar_item(999, "+".to_string(), false, true));

    // Settings button
    let settings_btn = sized_box(
        crate::terminal_keyboard::focusless_button(label("S"), |data: &mut App| {
            data.status_message = "Settings not implemented yet".to_string();
        })
    ).padding(4.0);

    sized_box(
        flex_col((
            flex_col(items).flex(1.0),
            settings_btn,
        ))
    )
    .fixed_width(Length::px(48.0))
    .background(MANTLE)
    .padding(4.0)
}

fn tab_item(idx: usize, text: String, is_active: bool) -> impl WidgetView<App> + use<> {
    let bg_color = if is_active {
        Color::from_rgba8(0x45, 0x47, 0x5a, 0xff) // Surface 1
    } else {
        Color::from_rgba8(0x1e, 0x1e, 0x2e, 0x00) // Transparent
    };

    // Style the tabs to look like a single integrated unit.
    // We use two buttons but remove the gap between them.
    flex_row((
        crate::terminal_keyboard::focusless_button(label(text), move |data: &mut App| {
            data.switch_tab(idx);
        })
        .padding(0.0), 
        crate::terminal_keyboard::focusless_button(label("x").text_size(11.0), move |data: &mut App| {
            data.remove_tab_at(idx);
        })
        .padding(0.0),
    ))
    .background(bg_color)
    .padding(8.0)
}

/// Tab bar with active group's tabs
fn tab_bar(state: &App) -> impl WidgetView<App> + use<> {
    let mut items = Vec::new();
    if let Some(group) = state.groups.get(state.active_group_index) {
        for (idx, tab) in group.tabs.iter().enumerate() {
            let pane_count = tab.pane_tree.pane_count();
            let label_text = if pane_count > 1 {
                format!(" {} [{}] ", tab.name, pane_count)
            } else {
                format!(" {} ", tab.name)
            };
            let is_active = idx == group.active_tab_index;
            items.push(tab_item(idx, label_text, is_active));
        }
    }

    let add_tab_btn = crate::terminal_keyboard::focusless_button(label(" + "), |data: &mut App| {
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
    .padding(2.0)
}

/// VSCode style status bar
fn status_bar(state: &App) -> impl WidgetView<App> + use<> {
    let left_info = if let Some(g) = state.active_group() {
        if let Some(t) = g.active_tab() {
            let (col, row) = state.cursor_pos();
            format!("Group: {} | Tab: {} | Panes: {} | Cursor: ({}, {})", g.name, t.name, t.pane_count(), col, row)
        } else {
            format!("Group: {}", g.name)
        }
    } else {
        "Ready".to_string()
    };
    
    let status = format!("{} | {} | Debug: {}", state.status_message, left_info, state.last_key_info);
    
    flex_row((
        label(status).text_size(12.0).color(Color::WHITE),
    ))
    .background(Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff))
    .padding(4.0)
}

/// Main application layout
fn app_logic(data: &mut App) -> impl WidgetView<App> + use<> {
    // Redraw trigger task to handle event-driven redraws
    let _redraw_trigger = task_raw(
        |proxy, state: &mut App| {
            state.register_redraw_proxy(proxy.clone());
            async move {
                println!("DEBUG: task_raw future spawned (now 100% event-driven)!");
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            }
        },
        |state: &mut App, ()| {
            // println!("DEBUG: task_raw event callback received message!");
            let before = state.redraw_counter;
            if state.check_and_clear_redraw() {
                state.redraw_counter += 1;
                println!("DEBUG: task_raw check_and_clear_redraw was TRUE! redraw_counter updated from {} to {}", before, state.redraw_counter);
            }
            state.apply_pending_resizes();
        },
    );

    let terminal_area = terminal_view::terminal_canvas_view(data.redraw_counter);
    let keyboard_capture = crate::terminal_keyboard::terminal_keyboard_view(|state: &mut App, action: crate::terminal_keyboard::TerminalAction| {
        match action {
            crate::terminal_keyboard::TerminalAction::Key(key_action) => {
                println!("DEBUG: Key received: key='{}', ctrl={}, alt={}, shift={}", key_action.key, key_action.ctrl, key_action.alt, key_action.shift);
                if !key_action.key.is_empty() {
                    state.handle_raw_key_event(&key_action.key, key_action.ctrl, key_action.alt, key_action.shift);
                    true
                } else {
                    false
                }
            }
            crate::terminal_keyboard::TerminalAction::Scroll { delta_lines } => {
                state.scroll_terminal(delta_lines);
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
        status_bar(data),
    )).cross_axis_alignment(CrossAxisAlignment::Stretch);

    // Root layout: sidebar | content
    let main_view = flex_row((
        sidebar(data),
        content.flex(1.0),
    )).cross_axis_alignment(CrossAxisAlignment::Stretch);

    fork(main_view, Some(_redraw_trigger))
}

fn main() {
    let app_state = App::new();

    let app = Xilem::new_simple(
        app_state,
        app_logic,
        WindowOptions::new("amux - Terminal Multiplexer [UI UPDATED]")
            .with_initial_inner_size(LogicalSize::new(1200.0, 800.0)),
    );

    app.run_in(EventLoop::with_user_event()).unwrap();
}
