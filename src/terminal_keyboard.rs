/// Terminal keyboard input widget
///
/// A custom Masonry widget that:
/// 1. Accepts focus and keyboard input
/// 2. Captures all keyboard events
/// 3. Routes prefix commands (Ctrl+B followed by c/d/n/p/x)
/// 4. Forwards other input to the active terminal's PTY using alacritty-style handling

use masonry::core::{
    Widget, WidgetId, EventCtx, PropertiesMut, PropertiesRef, TextEvent,
    RegisterCtx, LayoutCtx, MeasureCtx, PaintCtx, AccessCtx, ChildrenIds,
};
use masonry::kurbo::Size;
use masonry::imaging::Painter;
use accesskit::{Node, Role};
use masonry::layout::{LenReq, Axis};
use masonry::core::KeyboardEvent;
use winit::event::ElementState;
use winit::keyboard::{Key, NamedKey, ModifiersState};
use tracing::{Span, trace_span};

use crate::app::App;

/// Widget that captures keyboard input for terminal
pub struct TerminalKeyboardWidget {
    /// Callback to invoke when a key is pressed
    /// Returns true if the key was handled (suppresses further processing)
    on_key: Box<dyn Fn(&mut App, &KeyboardEvent) -> bool + Send + Sync>,
}

impl TerminalKeyboardWidget {
    pub fn new<F>(on_key: F) -> Self
    where
        F: Fn(&mut App, &KeyboardEvent) -> bool + Send + Sync + 'static,
    {
        Self {
            on_key: Box::new(on_key),
        }
    }
}

impl Widget for TerminalKeyboardWidget {
    type Action = ();

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
                if key_event.state != ElementState::Pressed {
                    return;
                }

                // Request focus to ensure we keep receiving keyboard events
                ctx.request_focus();

                // Submit action to notify Xilem of state change
                ctx.submit_action(());
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

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("TerminalKeyboardWidget", id = id.trace())
    }
}
