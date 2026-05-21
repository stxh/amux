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

        // Dynamically measure monospace cell dimensions from font metrics
        use xilem::masonry::parley::style::{StyleProperty, FontFamily, FontFamilyName, GenericFamily};
        let (cell_w, cell_h) = {
            let probe = "MMMMMMMMMM"; // 10 chars
            let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
            mb.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            mb.push_default(StyleProperty::FontSize(18.0));
            let mut ml = mb.build(probe);
            ml.break_all_lines(None);
            (ml.width() as f64 / 10.0, ml.height() as f64)
        };

        // 对 pane tree 中的所有 pane 计算布局
        let area = Rect::new(0.0, 0.0, available_size.width, available_size.height);
        let layouts = tab.pane_tree.layout(area);
        let active_pane_id = tab.active_pane_id;

        // 收集需要 resize 的 pane（先收集，避免借用冲突）
        let mut pending: Vec<(usize, u16, u16)> = Vec::new();

        for (pane_id, rect) in &layouts {
            if available_size.width > 0.0 && available_size.height > 0.0 {
                let new_cols = (rect.width / cell_w).floor() as u16;
                let new_rows = (rect.height / cell_h).floor() as u16;
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
                    fcx,
                    lcx,
                );
            }
        }

        // 把 resize 请求加入队列
        state.pending_resizes.extend(pending);
    }).alt_text("Terminal multiplexer output")
}
