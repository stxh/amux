/// Terminal keyboard input widget
///
/// A custom Masonry widget that:
/// 1. Accepts focus and keyboard input
/// 2. Captures all keyboard events
/// 3. Routes prefix commands (Ctrl+B followed by c/d/n/p/x)
/// 4. Forwards other input to the active terminal's PTY using alacritty-style handling

use std::sync::Mutex;
use xilem::masonry::core::{
    Widget, EventCtx, PropertiesMut, PropertiesRef, TextEvent, PointerEvent,
    RegisterCtx, LayoutCtx, MeasureCtx, PaintCtx, AccessCtx, ChildrenIds,
    WidgetPod, NewWidget, WidgetMut, AccessEvent, PointerScrollEvent, WidgetId,
};
use xilem::masonry::kurbo::{Size, Axis};
use xilem::masonry::imaging::Painter;
use accesskit::{Node, Role};
use xilem::masonry::layout::LenReq;

use crate::app::App;
use xilem::core::ViewPathTracker;

static TERMINAL_WIDGET_ID: Mutex<Option<WidgetId>> = Mutex::new(None);


#[derive(Debug, Clone)]
pub struct KeyAction {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone)]
pub enum TerminalAction {
    Key(KeyAction),
    Scroll { delta_lines: i32 },
    MouseDown { x: f64, y: f64, button: xilem::masonry::core::PointerButton },
    MouseMove { x: f64, y: f64, is_dragging: bool },
    MouseUp { x: f64, y: f64, button: xilem::masonry::core::PointerButton },
    /// IME committed text (e.g., from CJK input method)
    TextCommit(String),
}

pub struct TerminalKeyboardWidget {
    is_left_down: bool,
}

impl TerminalKeyboardWidget {
    pub fn new() -> Self {
        Self {
            is_left_down: false,
        }
    }
}

impl Widget for TerminalKeyboardWidget {
    type Action = TerminalAction;

    /// Allow this widget to receive focus
    fn accepts_focus(&self) -> bool {
        true
    }

    /// Allow this widget to receive keyboard events
    fn accepts_text_input(&self) -> bool {
        true
    }

    /// Handle pointer events including click-to-focus and mouse wheel scroll
    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        match event {
            PointerEvent::Down(pointer_event) => {
                ctx.request_focus();
                let physical = pointer_event.state.position;
                let pos = ctx.local_position(physical);
                let x = pos.x;
                let y = pos.y;
                let button = pointer_event.button;
                if button == Some(xilem::masonry::core::PointerButton::Primary) {
                    self.is_left_down = true;
                }
                if let Some(btn) = button {
                    ctx.submit_action::<TerminalAction>(TerminalAction::MouseDown { x, y, button: btn });
                }
            }
            PointerEvent::Up(pointer_event) => {
                let physical = pointer_event.state.position;
                let pos = ctx.local_position(physical);
                let x = pos.x;
                let y = pos.y;
                let button = pointer_event.button;
                if button == Some(xilem::masonry::core::PointerButton::Primary) {
                    self.is_left_down = false;
                }
                if let Some(btn) = button {
                    ctx.submit_action::<TerminalAction>(TerminalAction::MouseUp { x, y, button: btn });
                }
            }
            PointerEvent::Move(pointer_event) => {
                ctx.request_focus();
                let physical = pointer_event.current.position;
                let pos = ctx.local_position(physical);
                let x = pos.x;
                let y = pos.y;
                ctx.submit_action::<TerminalAction>(TerminalAction::MouseMove {
                    x,
                    y,
                    is_dragging: self.is_left_down,
                });
            }
            PointerEvent::Scroll(PointerScrollEvent { delta, .. }) => {
                ctx.request_focus();
                use xilem::masonry::core::ScrollDelta;
                let lines = match delta {
                    ScrollDelta::LineDelta(_x, y) => *y as i32,
                    ScrollDelta::PixelDelta(pos) => {
                        // Approximate: 40px per line
                        (pos.y / 40.0) as i32
                    }
                    ScrollDelta::PageDelta(_x, y) => {
                        // Page scroll: treat as ~20 lines per page
                        (*y as i32) * 20
                    }
                };
                if lines != 0 {
                    ctx.submit_action::<TerminalAction>(TerminalAction::Scroll { delta_lines: lines });
                    ctx.set_handled();
                }
            }
            _ => {}
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

        // println!("DEBUG: TerminalKeyboardWidget received text event: {:?}", event);
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
                        } else if s.chars().count() == 1 {
                            let c = s.chars().next().unwrap();
                            let val = c as u32;
                            if val >= 1 && val <= 26 {
                                // Map control character back to 'a'..='z'
                                let mapped = ((val - 1) + 'a' as u32) as u8 as char;
                                mapped.to_string()
                            } else {
                                s.to_string()
                            }
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
                    
                    // Check if it was originally a control character code
                    let is_control_char = match &key_event.key {
                        MasonryKey::Character(s) if s.chars().count() == 1 => {
                            let val = s.chars().next().unwrap() as u32;
                            val >= 1 && val <= 26
                        }
                        _ => false,
                    };

                    let action = KeyAction {
                        key: key_str,
                        ctrl: is_control_char || mods_dbg.contains("CONTROL") || mods_dbg.contains("CTRL"),
                        alt: mods_dbg.contains("ALT"),
                        shift: mods_dbg.contains("SHIFT"),
                    };
                    // println!("DEBUG: TerminalKeyboardWidget captured: {:?}", action);
                    ctx.submit_action::<TerminalAction>(TerminalAction::Key(action));
                }
            }
            TextEvent::Ime(xilem::masonry::core::Ime::Commit(text)) => {
                if !text.is_empty() {
                    ctx.submit_action::<TerminalAction>(TerminalAction::TextCommit(text.clone()));
                }
            }
            TextEvent::Ime(xilem::masonry::core::Ime::Preedit(_, _)) => {
                // IME composition in progress - ignore preedit rendering for now
                // The OS handles the composition window
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
        len_req: LenReq,
        _cross_length: Option<f64>,
    ) -> f64 {
        match len_req {
            LenReq::MinContent | LenReq::MaxContent => 0.0,
            LenReq::FitContent(space) => space,
        }
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
    F: Fn(&mut App, TerminalAction) -> bool + Send + Sync + 'static,
{
    TerminalKeyboardView { on_key }
}

impl<F> xilem::core::ViewMarker for TerminalKeyboardView<F> {}

impl<F> xilem::core::View<App, (), xilem::ViewCtx> for TerminalKeyboardView<F>
where
    F: Fn(&mut App, TerminalAction) -> bool + Send + Sync + 'static,
{
    type Element = xilem::Pod<TerminalKeyboardWidget>;
    type ViewState = ();

    fn build(&self, ctx: &mut xilem::ViewCtx, _state: &mut App) -> (Self::Element, Self::ViewState) {
        let widget = TerminalKeyboardWidget::new();
        let pod = ctx.with_action_widget(|ctx| {
            ctx.create_pod(widget)
        });
        if let Ok(mut lock) = TERMINAL_WIDGET_ID.lock() {
            *lock = Some(pod.new_widget.id());
        }
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
        if let Some(action) = message.take_message::<TerminalAction>() {
            if (self.on_key)(app_state, *action) {
                // Handled
            }
            xilem::core::MessageResult::Action(())
        } else {
            xilem::core::MessageResult::Stale
        }
    }
}

// ─── Focusless Button Widget & View ───

pub struct FocuslessButtonWidget {
    child: WidgetPod<dyn Widget>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct FocuslessButtonPress {
    pub button: Option<xilem::masonry::core::PointerButton>,
}

impl FocuslessButtonWidget {
    pub fn new(child: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            child: child.erased().to_pod(),
        }
    }
    
    pub fn set_child(this: &mut WidgetMut<'_, Self>, child: NewWidget<impl Widget + ?Sized>) {
        this.ctx.remove_child(std::mem::replace(
            &mut this.widget.child,
            child.erased().to_pod(),
        ));
    }

    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl Widget for FocuslessButtonWidget {
    type Action = FocuslessButtonPress;

    fn accepts_focus(&self) -> bool {
        false
    }

    fn accepts_text_input(&self) -> bool {
        false
    }

    fn propagates_pointer_interaction(&self) -> bool {
        false
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        match event {
            PointerEvent::Down(..) => {
                ctx.capture_pointer();
                ctx.request_paint_only();
            }
            PointerEvent::Up(pointer_event) => {
                if ctx.is_active() && ctx.is_hovered() {
                    ctx.submit_action::<Self::Action>(FocuslessButtonPress {
                        button: pointer_event.button,
                    });
                    if let Ok(lock) = TERMINAL_WIDGET_ID.lock() {
                        if let Some(terminal_id) = *lock {
                            ctx.set_focus(terminal_id);
                        }
                    }
                }
                ctx.request_paint_only();
            }
            _ => (),
        }
    }

    fn on_text_event(
        &mut self,
        _ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        _event: &TextEvent,
    ) {}

    fn on_access_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &AccessEvent,
    ) {
        match event.action {
            accesskit::Action::Click => {
                ctx.submit_action::<Self::Action>(FocuslessButtonPress { button: None });
                if let Ok(lock) = TERMINAL_WIDGET_ID.lock() {
                    if let Some(terminal_id) = *lock {
                        ctx.set_focus(terminal_id);
                    }
                }
            }
            _ => {}
        }
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        len_req: LenReq,
        cross_length: Option<f64>,
    ) -> f64 {
        use xilem::masonry::layout::LayoutSize;

        let auto_length = len_req.into();
        let context_size = LayoutSize::maybe(axis.cross(), cross_length);

        ctx.compute_length(
            &mut self.child,
            auto_length,
            context_size,
            axis,
            cross_length,
        )
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, size: Size) {
        let child_size = ctx.compute_size(&mut self.child, xilem::masonry::layout::SizeDef::fit(size), size.into());
        ctx.run_layout(&mut self.child, child_size);

        let child_origin = ((size - child_size).to_vec2() * 0.5).to_point();
        ctx.place_child(&mut self.child, child_origin);

        ctx.derive_baselines(&self.child);
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {}

    fn accessibility_role(&self) -> Role {
        Role::Button
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.add_action(accesskit::Action::Click);
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }
}

pub struct FocuslessButton<State, Action, F, V> {
    child: V,
    callback: F,
    phantom: std::marker::PhantomData<fn(State) -> Action>,
}

pub fn focusless_button<
    State: 'static,
    Action,
    V: xilem::WidgetView<State, Action>,
    F: Fn(&mut State) -> Action + Send + Sync + 'static,
>(
    child: V,
    callback: F,
) -> FocuslessButton<
    State,
    Action,
    impl Fn(&mut State, Option<xilem::masonry::core::PointerButton>) -> xilem::core::MessageResult<Action> + Send + Sync + 'static,
    V,
> {
    FocuslessButton {
        child,
        callback: move |state: &mut State, button| match button {
            None | Some(xilem::masonry::core::PointerButton::Primary) => {
                xilem::core::MessageResult::Action(callback(state))
            }
            _ => xilem::core::MessageResult::Nop,
        },
        phantom: std::marker::PhantomData,
    }
}

impl<State, Action, F, V> xilem::core::ViewMarker for FocuslessButton<State, Action, F, V> {}

const FOCUSLESS_BUTTON_CONTENT_VIEW_ID: xilem::core::ViewId = xilem::core::ViewId::new(0xf0c1c4bf);

impl<F, V, State, Action> xilem::core::View<State, Action, xilem::ViewCtx> for FocuslessButton<State, Action, F, V>
where
    State: 'static,
    Action: 'static,
    V: xilem::WidgetView<State, Action>,
    F: Fn(&mut State, Option<xilem::masonry::core::PointerButton>) -> xilem::core::MessageResult<Action> + Send + Sync + 'static,
{
    type Element = xilem::Pod<FocuslessButtonWidget>;
    type ViewState = V::ViewState;

    fn build(&self, ctx: &mut xilem::ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child, child_state) = ctx.with_id(FOCUSLESS_BUTTON_CONTENT_VIEW_ID, |ctx| {
            xilem::core::View::<State, Action, _>::build(&self.child, ctx, app_state)
        });
        (
            ctx.with_action_widget(|ctx| {
                let pod = ctx.create_pod(FocuslessButtonWidget::new(child.new_widget));
                pod
            }),
            child_state,
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        state: &mut Self::ViewState,
        ctx: &mut xilem::ViewCtx,
        mut element: xilem::core::Mut<'_, Self::Element>,
        app_state: &mut State,
    ) {
        ctx.with_id(FOCUSLESS_BUTTON_CONTENT_VIEW_ID, |ctx| {
            xilem::core::View::<State, Action, _>::rebuild(
                &self.child,
                &prev.child,
                state,
                ctx,
                FocuslessButtonWidget::child_mut(&mut element).downcast(),
                app_state,
            );
        });
    }

    fn teardown(
        &self,
        view_state: &mut Self::ViewState,
        ctx: &mut xilem::ViewCtx,
        mut element: xilem::core::Mut<'_, Self::Element>,
    ) {
        ctx.with_id(FOCUSLESS_BUTTON_CONTENT_VIEW_ID, |ctx| {
            xilem::core::View::<State, Action, _>::teardown(
                &self.child,
                view_state,
                ctx,
                FocuslessButtonWidget::child_mut(&mut element).downcast(),
            );
        });
        ctx.teardown_action_source(element);
    }

    fn message(
        &self,
        view_state: &mut Self::ViewState,
        message: &mut xilem::core::MessageCtx,
        mut element: xilem::core::Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> xilem::core::MessageResult<Action> {
        match message.take_first() {
            Some(FOCUSLESS_BUTTON_CONTENT_VIEW_ID) => self.child.message(
                view_state,
                message,
                FocuslessButtonWidget::child_mut(&mut element).downcast(),
                app_state,
            ),
            None => match message.take_message::<FocuslessButtonPress>() {
                Some(press) => (self.callback)(app_state, press.button),
                None => xilem::core::MessageResult::Stale,
            },
            _ => xilem::core::MessageResult::Stale,
        }
    }
}

// ─── DragArea Widget & View ───

pub struct DragAreaWidget;

impl Widget for DragAreaWidget {
    type Action = ();

    fn accepts_focus(&self) -> bool {
        false
    }

    fn accepts_text_input(&self) -> bool {
        false
    }

    fn on_pointer_event(
        &mut self,
        _ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        if let PointerEvent::Down(..) = event {
            // println!("DEBUG: DragAreaWidget clicked, dragging window");
            crate::win_util::drag_active_window();
        }
    }

    fn on_text_event(
        &mut self,
        _ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        _event: &TextEvent,
    ) {}

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn measure(
        &mut self,
        _ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        _axis: Axis,
        len_req: LenReq,
        _cross_length: Option<f64>,
    ) -> f64 {
        match len_req {
            LenReq::MinContent | LenReq::MaxContent => 0.0,
            LenReq::FitContent(space) => space,
        }
    }

    fn layout(&mut self, _ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, _size: Size) {}

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {}

    fn accessibility_role(&self) -> Role {
        Role::Unknown
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {}

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }
}

pub struct DragArea;

impl xilem::core::ViewMarker for DragArea {}

impl<State: 'static, Action> xilem::core::View<State, Action, xilem::ViewCtx> for DragArea {
    type Element = xilem::Pod<DragAreaWidget>;
    type ViewState = ();

    fn build(&self, ctx: &mut xilem::ViewCtx, _state: &mut State) -> (Self::Element, Self::ViewState) {
        let widget = DragAreaWidget;
        let pod = ctx.create_pod(widget);
        (pod, ())
    }

    fn rebuild(
        &self,
        _prev: &Self,
        _view_state: &mut Self::ViewState,
        _ctx: &mut xilem::ViewCtx,
        _element: xilem::core::Mut<'_, Self::Element>,
        _app_state: &mut State,
    ) {}

    fn teardown(
        &self,
        _view_state: &mut Self::ViewState,
        _ctx: &mut xilem::ViewCtx,
        _element: xilem::core::Mut<'_, Self::Element>,
    ) {
        // No teardown needed
    }

    fn message(
        &self,
        _view_state: &mut Self::ViewState,
        _message: &mut xilem::core::MessageCtx,
        _element: xilem::core::Mut<'_, Self::Element>,
        _app_state: &mut State,
    ) -> xilem::core::MessageResult<Action> {
        xilem::core::MessageResult::Nop
    }
}

pub fn drag_area() -> DragArea {
    DragArea
}
