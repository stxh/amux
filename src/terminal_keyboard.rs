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
        use xilem::masonry::core::keyboard::Key as MasonryKey;
        use xilem::masonry::core::keyboard::NamedKey as MasonryNamedKey;
        use xilem::masonry::core::keyboard::KeyState;

        println!("DEBUG: TerminalKeyboardWidget received text event: {:?}", event);
        match event {
            TextEvent::Keyboard(key_event) => {
                // Request focus again to be safe
                ctx.request_focus();

                // Only handle key-down events (ignore key-up and repeat for now)
                if key_event.state != KeyState::Down {
                    return;
                }

                let key_str = match &key_event.key {
                    MasonryKey::Character(s) => {
                        if s == " " {
                            "space".to_string()
                        } else {
                            s.to_string()
                        }
                    }
                    MasonryKey::Named(named) => {
                        match named {
                            MasonryNamedKey::Enter => "enter".to_string(),
                            MasonryNamedKey::Backspace => "backspace".to_string(),
                            MasonryNamedKey::Tab => "tab".to_string(),
                            MasonryNamedKey::Escape => "escape".to_string(),
                            MasonryNamedKey::ArrowUp => "up".to_string(),
                            MasonryNamedKey::ArrowDown => "down".to_string(),
                            MasonryNamedKey::ArrowLeft => "left".to_string(),
                            MasonryNamedKey::ArrowRight => "right".to_string(),
                            MasonryNamedKey::Home => "home".to_string(),
                            MasonryNamedKey::End => "end".to_string(),
                            MasonryNamedKey::PageUp => "pageup".to_string(),
                            MasonryNamedKey::PageDown => "pagedown".to_string(),
                            MasonryNamedKey::Insert => "insert".to_string(),
                            MasonryNamedKey::Delete => "delete".to_string(),
                            MasonryNamedKey::F1 => "f1".to_string(),
                            MasonryNamedKey::F2 => "f2".to_string(),
                            MasonryNamedKey::F3 => "f3".to_string(),
                            MasonryNamedKey::F4 => "f4".to_string(),
                            MasonryNamedKey::F5 => "f5".to_string(),
                            MasonryNamedKey::F6 => "f6".to_string(),
                            MasonryNamedKey::F7 => "f7".to_string(),
                            MasonryNamedKey::F8 => "f8".to_string(),
                            MasonryNamedKey::F9 => "f9".to_string(),
                            MasonryNamedKey::F10 => "f10".to_string(),
                            MasonryNamedKey::F11 => "f11".to_string(),
                            MasonryNamedKey::F12 => "f12".to_string(),
                            _ => "".to_string(),
                        }
                    }
                };

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
        let pod = ctx.with_action_widget(|ctx| {
            ctx.create_pod(widget)
        });
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
        ctx: &mut xilem::ViewCtx,
        element: xilem::core::Mut<'_, Self::Element>,
    ) {
        ctx.teardown_action_source(element);
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
