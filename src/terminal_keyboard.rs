/// Terminal keyboard input widget
///
/// A custom Masonry widget that:
/// 1. Accepts focus and keyboard input
/// 2. Captures all keyboard events
/// 3. Routes prefix commands (Ctrl+B followed by c/d/n/p/x)
/// 4. Forwards other input to the active terminal's PTY using alacritty-style handling

use xilem::masonry::core::{
    Widget, EventCtx, PropertiesMut, PropertiesRef, TextEvent, PointerEvent,
    RegisterCtx, LayoutCtx, MeasureCtx, PaintCtx, AccessCtx, ChildrenIds,
};
use xilem::masonry::kurbo::{Size, Axis};
use xilem::masonry::imaging::Painter;
use xilem::masonry::peniko::Color;
use accesskit::{Node, Role};
use xilem::masonry::layout::LenReq;

use crate::app::App;

#[derive(Debug, Clone)]
pub struct KeyAction {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

pub struct TerminalKeyboardWidget;

impl TerminalKeyboardWidget {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for TerminalKeyboardWidget {
    type Action = KeyAction;

    /// Allow this widget to receive focus
    fn accepts_focus(&self) -> bool {
        true
    }

    /// Allow this widget to receive keyboard events
    fn accepts_text_input(&self) -> bool {
        true
    }

    /// Handle keyboard events - this is where the magic happens
    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        let event_str = format!("{:?}", event);
        if event_str.contains("PointerDown") || event_str.contains("Down") {
            println!("DEBUG: TerminalKeyboardWidget clicked (event: {}), requesting focus", event_str);
            ctx.request_focus();
        }
    }

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        println!("DEBUG: TerminalKeyboardWidget received text event: {:?}", event);
        match event {
            TextEvent::Keyboard(key_event) => {
                // Request focus again to be safe
                ctx.request_focus();

                // Only handle key-down events (ignore key-up and repeat for now)
                let state_str = format!("{:?}", key_event.state);
                if state_str != "Pressed" {
                    return;
                }

                let key_dbg = format!("{:?}", key_event.key).to_lowercase();
                let mut key_str = String::new();
                
                // Better key parsing
                if key_dbg.contains("enter") {
                    key_str = "enter".to_string();
                } else if key_dbg.contains("backspace") {
                    key_str = "backspace".to_string();
                } else if key_dbg.contains("tab") {
                    key_str = "tab".to_string();
                } else if key_dbg.contains("escape") {
                    key_str = "escape".to_string();
                } else if key_dbg.contains("arrowup") || key_dbg.contains("up") {
                    key_str = "up".to_string();
                } else if key_dbg.contains("arrowdown") || key_dbg.contains("down") {
                    key_str = "down".to_string();
                } else if key_dbg.contains("arrowleft") || key_dbg.contains("left") {
                    key_str = "left".to_string();
                } else if key_dbg.contains("arrowright") || key_dbg.contains("right") {
                    key_str = "right".to_string();
                } else if let Some(start) = key_dbg.find("character(\"") {
                    if let Some(end) = key_dbg[start + 12..].find("\")") {
                        key_str = key_dbg[start + 12 .. start + 12 + end].to_string();
                    }
                } else if key_dbg.len() == 1 {
                    // Fallback for some environments
                    key_str = key_dbg;
                }

                if !key_str.is_empty() {
                    let modifiers = key_event.modifiers;
                    let mods_dbg = format!("{:?}", modifiers).to_uppercase();
                    let action = KeyAction {
                        key: key_str,
                        ctrl: mods_dbg.contains("CONTROL") || mods_dbg.contains("CTRL"),
                        alt: mods_dbg.contains("ALT"),
                        shift: mods_dbg.contains("SHIFT"),
                    };
                    println!("DEBUG: TerminalKeyboardWidget captured: {:?}", action);
                    ctx.submit_action::<KeyAction>(action);
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
        30.0 // Match the height in main.rs
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
        painter: &mut Painter<'_>,
    ) {
        // We won't draw anything for now to keep it clean, 
        // the background comes from the parent sized_box.
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
    F: Fn(&mut App, KeyAction) -> bool + Send + Sync + 'static,
{
    TerminalKeyboardView { on_key }
}

impl<F> xilem::core::ViewMarker for TerminalKeyboardView<F> {}

impl<F> xilem::core::View<App, (), xilem::ViewCtx> for TerminalKeyboardView<F>
where
    F: Fn(&mut App, KeyAction) -> bool + Send + Sync + 'static,
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
        if let Some(action) = message.take_message::<KeyAction>() {
            if (self.on_key)(app_state, *action) {
                // Handled
            }
            xilem::core::MessageResult::Action(())
        } else {
            xilem::core::MessageResult::Stale
        }
    }
}
