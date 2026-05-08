/// Terminal keyboard input widget
///
/// A custom Masonry widget that:
/// 1. Accepts focus and keyboard input
/// 2. Captures all keyboard events
/// 3. Routes prefix commands (Ctrl+B followed by c/d/n/p/x)
/// 4. Forwards other input to the active terminal's PTY using alacritty-style handling

use xilem::masonry::core::{
    Widget, EventCtx, PropertiesMut, PropertiesRef, TextEvent,
    RegisterCtx, LayoutCtx, MeasureCtx, PaintCtx, AccessCtx, ChildrenIds,
};
use xilem::masonry::kurbo::{Size, Axis};
use xilem::masonry::imaging::Painter;
use accesskit::{Node, Role};
use xilem::masonry::layout::LenReq;

use crate::app::App;

pub struct TerminalKeyboardWidget;

impl TerminalKeyboardWidget {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for TerminalKeyboardWidget {
    type Action = String;

    /// Allow this widget to receive focus
    fn accepts_focus(&self) -> bool {
        true
    }

    /// Allow this widget to receive keyboard events
    fn accepts_text_input(&self) -> bool {
        true
    }

    /// Handle keyboard events - this is where the magic happens
    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        match event {
            TextEvent::Keyboard(key_event) => {
                // Only handle key-down events (ignore key-up and repeat for now)
                if format!("{:?}", key_event.state) != "Pressed" {
                    return;
                }

                // Request focus to ensure we keep receiving keyboard events
                ctx.request_focus();

                let key_dbg = format!("{:?}", key_event.key).to_lowercase();
                let mut key_str = String::new();
                if key_dbg.contains("enter") {
                    key_str = "enter".to_string();
                } else if key_dbg.contains("backspace") {
                    key_str = "backspace".to_string();
                } else if key_dbg.contains("tab") {
                    key_str = "tab".to_string();
                } else if key_dbg.contains("escape") {
                    key_str = "escape".to_string();
                } else if key_dbg.contains("arrowup") {
                    key_str = "up".to_string();
                } else if key_dbg.contains("arrowdown") {
                    key_str = "down".to_string();
                } else if key_dbg.contains("arrowleft") {
                    key_str = "left".to_string();
                } else if key_dbg.contains("arrowright") {
                    key_str = "right".to_string();
                } else if key_dbg.contains("character(\"") {
                    if let Some(start) = key_dbg.find("character(\"") {
                        if let Some(end) = key_dbg[start + 12..].find("\")") {
                            key_str = key_dbg[start + 12 .. start + 12 + end].to_string();
                        }
                    }
                }

                // Submit action to notify Xilem of state change
                if !key_str.is_empty() {
                    ctx.submit_action::<String>(key_str);
                }
            }
            _ => {}
        }
    }

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn measure(
        &mut self,
        _ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        _axis: Axis,
        _len_req: LenReq,
        _cross_length: Option<f64>,
    ) -> f64 {
        0.0 // Zero size - this is just a keyboard capture target
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx<'_>,
        _props: &PropertiesRef<'_>,
        _size: Size,
    ) {
        // No layout needed - invisible widget
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {
        // Nothing to paint
    }

    fn accessibility_role(&self) -> Role {
        Role::Unknown
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }
}

pub struct TerminalKeyboardView<F> {
    on_key: F,
}

pub fn terminal_keyboard_view<F>(on_key: F) -> TerminalKeyboardView<F>
where
    F: Fn(&mut App, String) -> bool + Send + Sync + 'static,
{
    TerminalKeyboardView { on_key }
}

impl<F> xilem::core::ViewMarker for TerminalKeyboardView<F> {}

impl<F> xilem::core::View<App, (), xilem::ViewCtx> for TerminalKeyboardView<F>
where
    F: Fn(&mut App, String) -> bool + Send + Sync + 'static,
{
    type Element = xilem::Pod<TerminalKeyboardWidget>;
    type ViewState = ();

    fn build(&self, ctx: &mut xilem::ViewCtx, _state: &mut App) -> (Self::Element, Self::ViewState) {
        let widget = TerminalKeyboardWidget::new();
        let pod = ctx.create_pod(widget);
        (pod, ())
    }

    fn rebuild(
        &self,
        _prev: &Self,
        _view_state: &mut Self::ViewState,
        _ctx: &mut xilem::ViewCtx,
        _element: xilem::core::Mut<'_, Self::Element>,
        _app_state: &mut App,
    ) {
    }

    fn teardown(
        &self,
        _view_state: &mut Self::ViewState,
        _ctx: &mut xilem::ViewCtx,
        _element: xilem::core::Mut<'_, Self::Element>,
    ) {
    }

    fn message(
        &self,
        _view_state: &mut Self::ViewState,
        message: &mut xilem::core::MessageCtx,
        _element: xilem::core::Mut<'_, Self::Element>,
        app_state: &mut App,
    ) -> xilem::core::MessageResult<()> {
        if let Some(action) = message.take_message::<String>() {
            if (self.on_key)(app_state, *action) {
                // Handled
            }
            xilem::core::MessageResult::Action(())
        } else {
            xilem::core::MessageResult::Stale
        }
    }
}
