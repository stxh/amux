mod app;
mod keyboard;
mod pane;
mod terminal;
mod terminal_render;

use app::App;
use pane::LayoutMode;
use xilem::view::{button, canvas, flex_col, label};
use xilem::style::Style;
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};
use xilem::dpi::LogicalSize;

/// Status bar
fn status_bar() -> impl WidgetView<App> + use<> {
    label("Ctrl+B: prefix | c:split-v | %:split-h | n:next | p:prev | x:close | Scroll: mouse wheel / Shift+PgUp/PgDn")
}

/// Button bar for mouse-based actions
fn action_buttons() -> impl WidgetView<App> + use<> {
    flex_col((
        button(label("Split Vertical"), |data: &mut App| {
            data.split_pane(LayoutMode::Vertical);
        }),
        button(label("Split Horizontal"), |data: &mut App| {
            data.split_pane(LayoutMode::Horizontal);
        }),
        button(label("Next Pane"), |data: &mut App| {
            data.next_pane();
        }),
        button(label("Prev Pane"), |data: &mut App| {
            data.prev_pane();
        }),
        button(label("Close Pane"), |data: &mut App| {
            data.close_pane();
        }),
    ))
    .padding(8.0)
}

fn app_logic(data: &mut App) -> impl WidgetView<App> + use<> {
    let active_idx = data.active_pane_index;
    
    // Create a terminal canvas that renders ALL panes in their layout positions
    let terminal_area = canvas(move |state: &mut App, _ctx, scene, window_size| {
        let mut painter = xilem::masonry::imaging::Painter::new(scene);
        
        // Background
        let bg_rect = window_size.to_rect();
        painter.fill(&bg_rect, xilem::masonry::peniko::Color::from_rgba8(0x11, 0x11, 0x1b, 0xff)).draw();
        
        // Render all panes in their layout positions
        for (pane_idx, pane) in state.panes.iter().enumerate() {
            let pane_state = &state.pane_states[pane_idx];
            let layout = &pane.layout;
            
            // Calculate absolute position and size
            let x = layout.x * window_size.width;
            let y = layout.y * window_size.height;
            let w = layout.width * window_size.width;
            let h = layout.height * window_size.height;
            
            let pane_size = xilem::masonry::kurbo::Size::new(w, h);
            
            // Render the terminal with offset
            terminal_render::render_terminal_with_offset(
                &mut painter,
                &pane.terminal,
                pane_state,
                pane_idx == active_idx,
                pane_size,
                x,
                y,
            );
        }
    }).alt_text("Terminal multiplexer output");

    flex_col((
        terminal_area,
        status_bar(),
    ))
}

fn main() {
    let app_state = App::new();
    
    let app = Xilem::new_simple(
        app_state,
        app_logic,
        WindowOptions::new("amux - Terminal Multiplexer")
            .with_initial_inner_size(LogicalSize::new(1200.0, 800.0)),
    );
    
    app.run_in(EventLoop::with_user_event()).unwrap();
}
