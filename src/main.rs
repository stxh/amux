mod app;
mod keyboard;
mod pane;
mod terminal;
mod terminal_render;

use app::App;
use pane::LayoutMode;
use xilem::view::{button, canvas, flex_col, label, flex_row, sized_box};
use xilem::style::Style;
use xilem::masonry::layout::Length;
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};
use xilem::dpi::LogicalSize;
use xilem::masonry::peniko::Color;

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
                let text = format!("Pane {}/{}", state.active_pane_index + 1, state.panes.len());
                let (fcx, lcx) = ctx.text_contexts();
                
                let mut builder = lcx.ranged_builder(fcx, &text, 1.0, true);
                builder.push_default(xilem::masonry::parley::style::FontFamily::Single(
                    xilem::masonry::parley::style::FontFamilyName::Generic(
                        xilem::masonry::parley::style::GenericFamily::Monospace,
                    ),
                ));
                builder.push_default(xilem::masonry::parley::style::StyleProperty::FontSize(14.0));
                
                let mut layout = builder.build(&text);
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
    
    // Create a terminal canvas that renders ALL panes in their layout positions
    // The canvas receives the available size after flex layout subtracts tab_bar and status_bar
    let terminal_area = canvas(move |state: &mut App, ctx, scene, available_size| {
        let mut painter = xilem::masonry::imaging::Painter::new(scene);
        
        // Get text contexts for rendering terminal text
        let (fcx, lcx) = ctx.text_contexts();
        
        // Background - fill the entire available space
        let bg_rect = available_size.to_rect();
        painter.fill(&bg_rect, Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff)).draw(); // Catppuccin Base
        
        // Render all panes in their layout positions
        // available_size is already the correct size (window - tab_bar - status_bar)
        for (pane_idx, pane) in state.panes.iter().enumerate() {
            let pane_state = &state.pane_states[pane_idx];
            let layout = &pane.layout;
            
            // Calculate absolute position and size within the terminal area
            // layout uses normalized coordinates (0.0-1.0)
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

    // cmux-style layout using flex with spacers to control sizing
    // Tab bar: 28px fixed
    // Terminal: fills remaining space  
    // Status bar: auto size
    // Calculate terminal height dynamically based on window size
    // Window is 800px initially, tab_bar is 28px, status_bar is ~30px
    // So terminal should be about 800 - 28 - 30 = 742px
    // We'll use a reasonable default and let the canvas handle resizing
    // 
    // NOTE: This is a workaround for Canvas not implementing WidgetView/FlexItem.
    // Once Xilem adds proper flex factor support for canvas, we can use flex(1.0).
    const TERMINAL_HEIGHT: f64 = 700.0; // Good default for 800px window
    
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
