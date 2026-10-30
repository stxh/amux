//! Left activity-bar icon.
//!
//! Close to [`crate::terminal_keyboard::focusless_button`], with two extras the
//! activity bar needs: it reports which button was released (so a right click
//! can ask to close its workspace) and it reports pointer hover (so a tooltip
//! with the workspace title and the shell's current directory can follow it).
//!
//! Hover status is not a pointer event in Masonry; the update pass calls
//! [`Widget::update`] with [`Update::HoveredChanged`] when the pointer enters or
//! leaves, which is reported here as an action.

use xilem::masonry::core::{
    AccessCtx, AccessEvent, ChildrenIds, EventCtx, LayoutCtx, MeasureCtx, PaintCtx, PointerEvent,
    PropertiesMut, PropertiesRef, RegisterCtx, Update, UpdateCtx, Widget, WidgetMut, WidgetPod,
};
use xilem::masonry::imaging::Painter;
use xilem::masonry::kurbo::{Axis, Size};
use accesskit::{Node, Role};

use xilem::core::ViewPathTracker;

/// What the activity bar needs to know about one icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarIconEvent {
    /// Primary button released over the icon.
    Activate,
    /// Secondary button released over the icon.
    Secondary,
    /// The pointer entered the icon.
    Hover,
    /// The pointer left the icon.
    Unhover,
}

pub struct SidebarIconWidget {
    child: WidgetPod<dyn Widget>,
    hovered: bool,
}

impl SidebarIconWidget {
    pub fn new(child: xilem::masonry::core::NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            child: child.erased().to_pod(),
            hovered: false,
        }
    }

    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl Widget for SidebarIconWidget {
    type Action = SidebarIconEvent;

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

    fn update(
        &mut self,
        ctx: &mut UpdateCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        update: &Update,
    ) {
        if let Update::HoveredChanged(hovered) = update {
            if *hovered != self.hovered {
                self.hovered = *hovered;
                ctx.submit_action::<Self::Action>(if *hovered {
                    SidebarIconEvent::Hover
                } else {
                    SidebarIconEvent::Unhover
                });
            }
        }
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
                    let action = if pointer_event.button
                        == Some(xilem::masonry::core::PointerButton::Secondary)
                    {
                        SidebarIconEvent::Secondary
                    } else {
                        SidebarIconEvent::Activate
                    };
                    ctx.submit_action::<Self::Action>(action);
                    // Hand keyboard focus back to the terminal, so Esc and the
                    // hotkeys keep working after clicking the activity bar.
                    if let Some(terminal_id) = crate::terminal_keyboard::terminal_widget_id() {
                        ctx.set_focus(terminal_id);
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
        _event: &xilem::masonry::core::TextEvent,
    ) {
    }

    fn on_access_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &AccessEvent,
    ) {
        if event.action == accesskit::Action::Click {
            ctx.submit_action::<Self::Action>(SidebarIconEvent::Activate);
        }
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        len_req: xilem::masonry::layout::LenReq,
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
        let child_size = ctx.compute_size(
            &mut self.child,
            xilem::masonry::layout::SizeDef::fit(size),
            size.into(),
        );
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
    ) {
    }

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

pub struct SidebarIcon<State, Action, F, V> {
    child: V,
    callback: F,
    phantom: std::marker::PhantomData<fn(State) -> Action>,
}

/// An activity-bar icon: `child` is the glyph, `callback` receives the icon's
/// events (activate, secondary click, hover).
pub fn sidebar_icon<State, Action, V, F>(child: V, callback: F) -> SidebarIcon<State, Action, F, V>
where
    State: 'static,
    Action: 'static,
    V: xilem::WidgetView<State, Action>,
    F: Fn(&mut State, SidebarIconEvent) -> xilem::core::MessageResult<Action>
        + Send
        + Sync
        + 'static,
{
    SidebarIcon {
        child,
        callback,
        phantom: std::marker::PhantomData,
    }
}

impl<State, Action, F, V> xilem::core::ViewMarker for SidebarIcon<State, Action, F, V> {}

const SIDEBAR_ICON_CONTENT_VIEW_ID: xilem::core::ViewId = xilem::core::ViewId::new(0x5b1de1c0);

impl<F, V, State, Action> xilem::core::View<State, Action, xilem::ViewCtx>
    for SidebarIcon<State, Action, F, V>
where
    State: 'static,
    Action: 'static,
    V: xilem::WidgetView<State, Action>,
    F: Fn(&mut State, SidebarIconEvent) -> xilem::core::MessageResult<Action>
        + Send
        + Sync
        + 'static,
{
    type Element = xilem::Pod<SidebarIconWidget>;
    type ViewState = V::ViewState;

    fn build(
        &self,
        ctx: &mut xilem::ViewCtx,
        app_state: &mut State,
    ) -> (Self::Element, Self::ViewState) {
        let (child, child_state) = ctx.with_id(SIDEBAR_ICON_CONTENT_VIEW_ID, |ctx| {
            xilem::core::View::<State, Action, _>::build(&self.child, ctx, app_state)
        });
        (
            ctx.with_action_widget(|ctx| ctx.create_pod(SidebarIconWidget::new(child.new_widget))),
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
        ctx.with_id(SIDEBAR_ICON_CONTENT_VIEW_ID, |ctx| {
            xilem::core::View::<State, Action, _>::rebuild(
                &self.child,
                &prev.child,
                state,
                ctx,
                SidebarIconWidget::child_mut(&mut element).downcast(),
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
        ctx.with_id(SIDEBAR_ICON_CONTENT_VIEW_ID, |ctx| {
            xilem::core::View::<State, Action, _>::teardown(
                &self.child,
                view_state,
                ctx,
                SidebarIconWidget::child_mut(&mut element).downcast(),
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
            Some(SIDEBAR_ICON_CONTENT_VIEW_ID) => self.child.message(
                view_state,
                message,
                SidebarIconWidget::child_mut(&mut element).downcast(),
                app_state,
            ),
            None => match message.take_message::<SidebarIconEvent>() {
                Some(event) => (self.callback)(app_state, *event),
                None => xilem::core::MessageResult::Stale,
            },
            _ => xilem::core::MessageResult::Stale,
        }
    }
}
