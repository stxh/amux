/// 终端视图模块
///
/// 策略：canvas 直接作为 flex 子元素填充可用空间。
/// text_input 单独放在布局中用于键盘捕获（不用 zstack，避免尺寸问题）。

use crate::app::App;
use crate::pane_tree::Rect;
use xilem::view::canvas;
use xilem::WidgetView;
use xilem::masonry::peniko::Color;

/// 创建终端 canvas 视图（不包含键盘捕获层）
/// 键盘捕获的 text_input 在 main.rs 的布局中单独添加
pub fn terminal_canvas_view(redraw_counter: u64) -> impl WidgetView<App> + use<> {
    canvas(move |state: &mut App, ctx, scene, available_size| {
        // Capture redraw_counter to force closure state change and widget rebuild
        let _ = redraw_counter;
        // Track window size for config saving
        state.window_size = (available_size.width, available_size.height);
        use xilem::masonry::imaging::Painter;
        let mut painter = Painter::new(scene);

        // 填充背景
        let bg_rect = available_size.to_rect();
        painter.fill(&bg_rect, Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff)).draw();

        // 获取当前激活的 group 和 tab
        let Some(group) = state.groups.get(state.active_group_index) else {
            return;
        };
        let Some(tab) = group.tabs.get(group.active_tab_index) else {
            return;
        };

        // 获取文本渲染上下文
        let (fcx, lcx) = ctx.text_contexts();

        // Dynamically measure monospace cell dimensions from font metrics.
        // Use a mixed Latin+CJK probe for cell_h so CJK fallback fonts
        // (e.g. PingFang on macOS) with taller metrics are accommodated,
        // preventing Chinese glyph overflow in selection highlighting.
        let font_size = state.font_size as f32;
        use xilem::masonry::parley::style::{StyleProperty, FontFamily, FontFamilyName, GenericFamily};
        let cell_w = {
            let probe = "MMMMMMMMMM";
            let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
            mb.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            mb.push_default(StyleProperty::FontSize(font_size));
            let mut ml = mb.build(probe);
            ml.break_all_lines(None);
            ml.width() as f64 / 10.0
        };
        let cell_h = {
            let probe = "MMMMMMMMMM你好";
            let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
            mb.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            mb.push_default(StyleProperty::FontSize(font_size));
            let mut ml = mb.build(probe);
            ml.break_all_lines(None);
            ml.height() as f64
        };
        state.cell_w = cell_w;
        state.cell_h = cell_h;

        // 对 pane tree 中的所有 pane 计算布局
        let area = Rect::new(0.0, 0.0, available_size.width, available_size.height);
        let layouts = tab.pane_tree.layout(area);
        state.last_pane_layouts = layouts.clone();
        let active_pane_id = tab.active_pane_id;

        // 收集需要 resize 的 pane（先收集，避免借用冲突）
        let mut pending: Vec<(usize, u16, u16)> = Vec::new();

        const MARGIN: f64 = 12.0;

        for (pane_id, rect) in &layouts {
            if available_size.width > 0.0 && available_size.height > 0.0 {
                let new_cols = ((rect.width - 2.0 * MARGIN).max(0.0) / cell_w).floor() as u16;
                let new_rows = ((rect.height - 2.0 * MARGIN).max(0.0) / cell_h).floor() as u16;
                let new_cols = new_cols.max(2);
                let new_rows = new_rows.max(2);

                if let Some((pane, _)) = tab.pane_tree.find(*pane_id) {
                    let current_cols = pane.terminal.grid_cols() as u16;
                    let current_rows = pane.terminal.grid_rows() as u16;
                    if new_cols != current_cols || new_rows != current_rows {
                        pending.push((*pane_id, new_cols, new_rows));
                    }
                }
            }

            if let Some((pane, pane_state)) = tab.pane_tree.find(*pane_id) {
                let pane_size = xilem::masonry::kurbo::Size::new(rect.width, rect.height);
                let is_active = *pane_id == active_pane_id;

                crate::terminal_render::render_terminal_with_offset(
                    &mut painter,
                    &pane.terminal,
                    pane_state,
                    is_active,
                    pane_size,
                    rect.x,
                    rect.y,
                    font_size,
                    fcx,
                    lcx,
                );

                // Draw floating close button overlay on hover (only if split pane count > 1)
                let pane_count = layouts.len();
                if pane_count > 1 {
                    let cb_x_min = rect.x + rect.width - 32.0;
                    let cb_x_max = rect.x + rect.width;
                    let cb_y_min = rect.y;
                    let cb_y_max = rect.y + 32.0;

                    let is_hovering_close = if let Some((hx, hy)) = state.hover_pos {
                        hx >= cb_x_min && hx <= cb_x_max && hy >= cb_y_min && hy <= cb_y_max
                    } else {
                        false
                    };

                    if is_hovering_close {
                        // Draw close button circle
                        let circle_center = xilem::masonry::kurbo::Point::new(rect.x + rect.width - 16.0, rect.y + 16.0);
                        let circle = xilem::masonry::kurbo::Circle::new(circle_center, 12.0);
                        painter.fill(&circle, Color::from_rgba8(235, 78, 78, 255)).draw(); // vibrant red

                        // Draw '✕' lines
                        let stroke = xilem::masonry::kurbo::Stroke::new(2.0);
                        let line1 = xilem::masonry::kurbo::Line::new(
                            xilem::masonry::kurbo::Point::new(rect.x + rect.width - 21.0, rect.y + 11.0),
                            xilem::masonry::kurbo::Point::new(rect.x + rect.width - 11.0, rect.y + 21.0),
                        );
                        let line2 = xilem::masonry::kurbo::Line::new(
                            xilem::masonry::kurbo::Point::new(rect.x + rect.width - 11.0, rect.y + 11.0),
                            xilem::masonry::kurbo::Point::new(rect.x + rect.width - 21.0, rect.y + 21.0),
                        );
                        painter.stroke(&line1, &stroke, Color::WHITE).draw();
                        painter.stroke(&line2, &stroke, Color::WHITE).draw();
                    }
                }
            }
        }

        // 把 resize 请求加入队列
        state.pending_resizes.extend(pending);
    }).alt_text("Terminal multiplexer output")
}
