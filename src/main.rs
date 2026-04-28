mod app;
mod keyboard;
mod pane;
mod terminal;
mod terminal_render;

use app::App;
use pane::LayoutMode;
use xilem::view::{button, canvas, flex_col, label, flex_row, sized_box, task_raw};
use xilem::style::Style;
use xilem::masonry::layout::Length;
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};
use xilem::dpi::LogicalSize;
use xilem::masonry::peniko::Color;
use alacritty_terminal::grid::Dimensions;
use std::time::Duration;

/// Tab bar with pane indicator and action buttons
fn tab_bar() -> impl WidgetView<App> + use<> {
    let bg_color = Color::from_rgba8(0x31, 0x32, 0x44, 0xff); // Catppuccin Surface0
    
    flex_row((
        // Left side: current pane indicator (dynamic canvas)
        sized_box(
            canvas(move |state: &mut App, ctx, scene, size| {
                let mut painter = xilem::masonry::imaging::Painter::new(scene);
                let rect = size.to_rect();
                painter.fill(&rect, bg_color).draw();
                
                // Draw pane indicator text using parley
                let (fcx, lcx) = ctx.text_contexts();
                
                // Debug info about terminal state
                let debug_text = if let Some(pane) = state.active_pane() {
                    let grid = pane.terminal.grid();
                    let cols = grid.columns();
                    let rows = grid.screen_lines();
                    let total = grid.total_lines();
                    let cursor = pane.terminal.cursor();
                    let disp_offset = pane.terminal.display_offset();
                    // Check content of a few rows
                    use alacritty_terminal::index::Line;
                    use alacritty_terminal::index::Column;
                    let row0_text: String = (0..cols.min(20)).map(|c| {
                        let cell = &grid[Line(0)][Column(c)];
                        if cell.c.is_control() { ' ' } else { cell.c }
                    }).collect();
                    let row23_text: String = (0..cols.min(20)).map(|c| {
                        let cell = &grid[Line(23)][Column(c)];
                        if cell.c.is_control() { ' ' } else { cell.c }
                    }).collect();
                    format!("P{}/{}|{}x{}|tot{}|do{}|c@{},{}|r0:'{}'|r23:'{}'", 
                            state.active_pane_index + 1, state.panes.len(),
                            cols, rows, total, disp_offset,
                            cursor.line.0, cursor.column.0,
                            row0_text.trim_end(), row23_text.trim_end())
                } else {
                    format!("Pane {}/{}", state.active_pane_index + 1, state.panes.len())
                };
                
                let mut builder = lcx.ranged_builder(fcx, &debug_text, 1.0, true);
                builder.push_default(xilem::masonry::parley::style::FontFamily::Single(
                    xilem::masonry::parley::style::FontFamilyName::Generic(
                        xilem::masonry::parley::style::GenericFamily::Monospace,
                    ),
                ));
                builder.push_default(xilem::masonry::parley::style::StyleProperty::FontSize(14.0));
                
                let mut layout = builder.build(&debug_text);
                layout.break_all_lines(None);
                layout.align(None, xilem::masonry::parley::Alignment::Start, 
                    xilem::masonry::parley::AlignmentOptions::default());
                
                let transform = xilem::masonry::kurbo::Affine::translate((8.0, 5.0));
                xilem::masonry::core::render_text(&mut painter, transform, &layout, &[xilem::masonry::peniko::Brush::Solid(
                    Color::from_rgba8(0xc6, 0xd0, 0xf5, 0xff)
                )], true);
            }).alt_text("Pane indicator")
        ).fixed_width(Length::px(150.0)).fixed_height(Length::px(28.0)),
        
        // Right side: action buttons
        button(label("+"), |data: &mut App| {
            data.split_pane(LayoutMode::Vertical);
        }),
        button(label("-"), |data: &mut App| {
            data.split_pane(LayoutMode::Horizontal);
        }),
        button(label("<"), |data: &mut App| {
            data.prev_pane();
        }),
        button(label(">"), |data: &mut App| {
            data.next_pane();
        }),
    ))
    .background(bg_color)
}

/// Status bar showing keyboard shortcuts
fn status_bar() -> impl WidgetView<App> + use<> {
    label("Ctrl+B: prefix | c:split-v | d:split-h | n:next | p:prev | x:close | Shift+PgUp/PgDn: scroll")
        .padding(4.0)
}

/// Main application layout with cmux-style design
fn app_logic(data: &mut App) -> impl WidgetView<App> + use<> {
    let active_idx = data.active_pane_index;
    
    // Periodic task to force redraw - terminals update asynchronously
    // This task runs every 100ms and increments a counter to trigger view rebuild
    let redraw_trigger = task_raw(
        |proxy, _state: &mut App| async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                // Send a tick message to trigger rebuild
                let _ = proxy.message(());
            }
        },
        |state: &mut App, ()| {
            // Increment redraw counter to trigger view rebuild
            state.redraw_counter += 1;
        },
    );

    // Create a terminal canvas that renders ALL panes in their layout positions
    let terminal_area = canvas(move |state: &mut App, ctx, scene, available_size| {
        let mut painter = xilem::masonry::imaging::Painter::new(scene);
        
        // Get text contexts for rendering terminal text
        let (fcx, lcx) = ctx.text_contexts();
        
        // Background - fill the entire available space
        let bg_rect = available_size.to_rect();
        painter.fill(&bg_rect, Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff)).draw(); // Catppuccin Base
        
        // Render all panes in their layout positions
        for (pane_idx, pane) in state.panes.iter().enumerate() {
            let pane_state = &state.pane_states[pane_idx];
            let layout = &pane.layout;
            
            // Calculate absolute position and size within the terminal area
            let x = layout.x * available_size.width;
            let y = layout.y * available_size.height;
            let w = layout.width * available_size.width;
            let h = layout.height * available_size.height;
            
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
                fcx,
                lcx,
            );
        }
    }).alt_text("Terminal multiplexer output");

    // cmux-style layout
    const TERMINAL_HEIGHT: f64 = 700.0;
    
    let terminal_box = sized_box(terminal_area)
        .fixed_height(Length::px(TERMINAL_HEIGHT));
    
    flex_col((
        tab_bar(),
        terminal_box,
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
