use std::borrow::Cow;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::Term;
use alacritty_terminal::grid::Scroll;
use winit::event::Modifiers;

use crate::alacritty::event::{Mouse, TouchPurpose, InlineSearchState};
use crate::alacritty::display::{Display, SizeInfo};
use crate::alacritty::config::UiConfig;
use crate::alacritty::clipboard::Clipboard;
use crate::alacritty::scheduler::Scheduler;
use crate::alacritty::message_bar::Message;

pub trait ActionContext<T: EventListener> {
    fn write_to_pty<B: Into<Cow<'static, [u8]>>>(&self, _data: B);
    fn mark_dirty(&mut self);
    fn size_info(&self) -> SizeInfo;
    fn copy_selection(&mut self, _ty: alacritty_terminal::term::ClipboardType);
    fn start_selection(&mut self, _ty: SelectionType, _point: Point, _side: Side);
    fn toggle_selection(&mut self, _ty: SelectionType, _point: Point, _side: Side);
    fn update_selection(&mut self, _point: Point, _side: Side);
    fn clear_selection(&mut self);
    fn selection_is_empty(&self) -> bool;
    fn mouse_mut(&mut self) -> &mut Mouse;
    fn mouse(&self) -> &Mouse;
    fn touch_purpose(&mut self) -> &mut TouchPurpose;
    fn modifiers(&mut self) -> &mut Modifiers;
    fn scroll(&mut self, _scroll: Scroll);
    fn display(&mut self) -> &mut Display;
    fn terminal(&self) -> &Term<T>;
    fn terminal_mut(&mut self) -> &mut Term<T>;
    fn config(&self) -> &UiConfig;
    fn clipboard_mut(&mut self) -> &mut Clipboard;
    fn scheduler_mut(&mut self) -> &mut Scheduler;
    fn search_active(&self) -> bool;
    fn inline_search_state(&mut self) -> &mut InlineSearchState;
}

pub struct Processor<T: EventListener, A: ActionContext<T>> {
    pub ctx: A,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: EventListener, A: ActionContext<T>> Processor<T, A> {
    pub fn new(ctx: A) -> Self {
        Self { ctx, _phantom: std::marker::PhantomData }
    }
    
    pub fn handle_event(&mut self, event: winit::event::WindowEvent) {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::KeyboardInput { event, .. } => self.key_input(event),
            WindowEvent::CursorMoved { position, .. } => self.mouse_moved(position),
            WindowEvent::MouseInput { state, button, .. } => {
                if state == winit::event::ElementState::Pressed {
                    self.on_mouse_press(button);
                } else {
                    self.on_mouse_release(button);
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => self.mouse_wheel_input(delta, phase),
            _ => (),
        }
    }

    pub fn handle_text_event(&mut self, event: xilem::masonry::core::TextEvent) {
        use xilem::masonry::core::keyboard::Key as MasonryKey;
        use xilem::masonry::core::keyboard::NamedKey as MasonryNamedKey;
        use xilem::masonry::core::keyboard::KeyState;
        use winit::keyboard::{Key, NamedKey};

        match event {
            xilem::masonry::core::TextEvent::Keyboard(key_event) => {
                let state = match key_event.state {
                    KeyState::Down => winit::event::ElementState::Pressed,
                    KeyState::Up => winit::event::ElementState::Released,
                };

                let (key, text) = match &key_event.key {
                    MasonryKey::Character(s) => (Key::Character(s.clone().into()), Some(s.clone())),
                    MasonryKey::Named(named) => {
                        let winit_named = match named {
                            MasonryNamedKey::Enter => NamedKey::Enter,
                            MasonryNamedKey::Backspace => NamedKey::Backspace,
                            MasonryNamedKey::Tab => NamedKey::Tab,
                            MasonryNamedKey::Escape => NamedKey::Escape,
                            MasonryNamedKey::ArrowUp => NamedKey::ArrowUp,
                            MasonryNamedKey::ArrowDown => NamedKey::ArrowDown,
                            MasonryNamedKey::ArrowLeft => NamedKey::ArrowLeft,
                            MasonryNamedKey::ArrowRight => NamedKey::ArrowRight,
                            MasonryNamedKey::Home => NamedKey::Home,
                            MasonryNamedKey::End => NamedKey::End,
                            MasonryNamedKey::PageUp => NamedKey::PageUp,
                            MasonryNamedKey::PageDown => NamedKey::PageDown,
                            MasonryNamedKey::Insert => NamedKey::Insert,
                            MasonryNamedKey::Delete => NamedKey::Delete,
                            MasonryNamedKey::F1 => NamedKey::F1,
                            MasonryNamedKey::F2 => NamedKey::F2,
                            MasonryNamedKey::F3 => NamedKey::F3,
                            MasonryNamedKey::F4 => NamedKey::F4,
                            MasonryNamedKey::F5 => NamedKey::F5,
                            MasonryNamedKey::F6 => NamedKey::F6,
                            MasonryNamedKey::F7 => NamedKey::F7,
                            MasonryNamedKey::F8 => NamedKey::F8,
                            MasonryNamedKey::F9 => NamedKey::F9,
                            MasonryNamedKey::F10 => NamedKey::F10,
                            MasonryNamedKey::F11 => NamedKey::F11,
                            MasonryNamedKey::F12 => NamedKey::F12,
                            _ => return, // skip unmapped named keys
                        };
                        (Key::Named(winit_named), None)
                    }
                    _ => (Key::Unidentified(winit::keyboard::NativeKey::Unidentified), None),
                };

                self.process_key(state, key, text);
            }
            _ => (),
        }
    }

    pub fn key_input(&mut self, event: winit::event::KeyEvent) {
        self.process_key(
            event.state,
            event.logical_key,
            event.text.map(|s| s.to_string()),
        );
    }

    pub fn process_key(&mut self, state: winit::event::ElementState, key: winit::keyboard::Key, text: Option<String>) {
        eprintln!("[amux] Processor::process_key state={state:?} key={key:?} text={text:?}");
        if state == winit::event::ElementState::Released {
            return;
        }

        let mode = *self.ctx.terminal().mode();
        let mods = self.ctx.modifiers().state();
        let text_str = text.as_deref().unwrap_or("");

        // Build sequence or use raw text
        let bytes = if self.should_build_sequence(&key, text_str, mode, mods) {
            eprintln!("[amux] Processor: building sequence for key={key:?}");
            self.build_sequence(key, mods, mode)
        } else {
            eprintln!("[amux] Processor: using raw text text_str={text_str:?}");
            let mut bytes = Vec::with_capacity(text_str.len() + 1);
            if mods.alt_key() {
                bytes.push(b'\x1b');
            }
            bytes.extend_from_slice(text_str.as_bytes());
            bytes
        };
        eprintln!("[amux] Processor: bytes to send: {bytes:?}");
        self.ctx.write_to_pty(bytes);
    }

    fn should_build_sequence(&self, key: &winit::keyboard::Key, text: &str, _mode: alacritty_terminal::term::TermMode, _mods: winit::keyboard::ModifiersState) -> bool {
        // Simple heuristic for now: if no text was provided or it's a named key that usually has a sequence
        text.is_empty() || matches!(key, winit::keyboard::Key::Named(_))
    }

    fn build_sequence(&self, key: winit::keyboard::Key, _mods: winit::keyboard::ModifiersState, _mode: alacritty_terminal::term::TermMode) -> Vec<u8> {
        // This is a complex function, I'll provide a simplified version or port the full one later
        // For now, use a basic mapping for arrows etc.
        use winit::keyboard::{Key, NamedKey};
        match key {
            Key::Named(NamedKey::ArrowUp) => b"\x1b[A".to_vec(),
            Key::Named(NamedKey::ArrowDown) => b"\x1b[B".to_vec(),
            Key::Named(NamedKey::ArrowRight) => b"\x1b[C".to_vec(),
            Key::Named(NamedKey::ArrowLeft) => b"\x1b[D".to_vec(),
            Key::Named(NamedKey::Home) => b"\x1b[H".to_vec(),
            Key::Named(NamedKey::End) => b"\x1b[F".to_vec(),
            Key::Named(NamedKey::PageUp) => b"\x1b[5~".to_vec(),
            Key::Named(NamedKey::PageDown) => b"\x1b[6~".to_vec(),
            Key::Named(NamedKey::Insert) => b"\x1b[2~".to_vec(),
            Key::Named(NamedKey::Delete) => b"\x1b[3~".to_vec(),
            Key::Named(NamedKey::F1) => b"\x1bOP".to_vec(),
            Key::Named(NamedKey::F2) => b"\x1bOQ".to_vec(),
            Key::Named(NamedKey::F3) => b"\x1bOR".to_vec(),
            Key::Named(NamedKey::F4) => b"\x1bOS".to_vec(),
            Key::Named(NamedKey::F5) => b"\x1b[15~".to_vec(),
            _ => Vec::new(),
        }
    }

    pub fn mouse_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        let size_info = self.ctx.size_info();
        let (x, y) = (position.x as usize, position.y as usize);

        self.ctx.mouse_mut().x = x;
        self.ctx.mouse_mut().y = y;

        let inside_text_area = size_info.contains_point(x, y);
        self.ctx.mouse_mut().inside_text_area = inside_text_area;

        // Update cell side
        let cell_x = (x as f32 - size_info.padding_x()) % size_info.cell_width();
        self.ctx.mouse_mut().cell_side = if cell_x > size_info.cell_width() / 2.0 {
            Side::Right
        } else {
            Side::Left
        };

        if self.ctx.mouse().left_button_state == winit::event::ElementState::Pressed {
            let display_offset = self.ctx.terminal().grid().display_offset();
            let point = self.ctx.mouse().point(&size_info, display_offset);
            self.ctx.update_selection(point, self.ctx.mouse().cell_side);
        }
    }
    pub fn on_mouse_press(&mut self, button: winit::event::MouseButton) {
        let size_info = self.ctx.size_info();
        let display_offset = self.ctx.terminal().grid().display_offset();
        let point = self.ctx.mouse().point(&size_info, display_offset);
        let side = self.ctx.mouse().cell_side;

        match button {
            winit::event::MouseButton::Left => {
                self.ctx.mouse_mut().left_button_state = winit::event::ElementState::Pressed;
                
                // Multi-click logic simplified
                let now = std::time::Instant::now();
                let elapsed = now - self.ctx.mouse().last_click_timestamp;
                self.ctx.mouse_mut().last_click_timestamp = now;

                self.ctx.mouse_mut().click_state = match self.ctx.mouse().click_state {
                    crate::alacritty::event::ClickState::Click if elapsed < std::time::Duration::from_millis(400) => crate::alacritty::event::ClickState::DoubleClick,
                    crate::alacritty::event::ClickState::DoubleClick if elapsed < std::time::Duration::from_millis(400) => crate::alacritty::event::ClickState::TripleClick,
                    _ => crate::alacritty::event::ClickState::Click,
                };

                self.ctx.clear_selection();
                let ty = match self.ctx.mouse().click_state {
                    crate::alacritty::event::ClickState::DoubleClick => SelectionType::Semantic,
                    crate::alacritty::event::ClickState::TripleClick => SelectionType::Lines,
                    _ => SelectionType::Simple,
                };
                self.ctx.start_selection(ty, point, side);
            }
            winit::event::MouseButton::Middle => {
                self.ctx.mouse_mut().middle_button_state = winit::event::ElementState::Pressed;
            }
            winit::event::MouseButton::Right => {
                self.ctx.mouse_mut().right_button_state = winit::event::ElementState::Pressed;
            }
            _ => (),
        }
    }

    pub fn on_mouse_release(&mut self, button: winit::event::MouseButton) {
        match button {
            winit::event::MouseButton::Left => {
                self.ctx.mouse_mut().left_button_state = winit::event::ElementState::Released;
                self.ctx.copy_selection(alacritty_terminal::term::ClipboardType::Selection);
            }
            winit::event::MouseButton::Middle => {
                self.ctx.mouse_mut().middle_button_state = winit::event::ElementState::Released;
            }
            winit::event::MouseButton::Right => {
                self.ctx.mouse_mut().right_button_state = winit::event::ElementState::Released;
            }
            _ => (),
        }
    }
    pub fn mouse_wheel_input(&mut self, delta: winit::event::MouseScrollDelta, _phase: winit::event::TouchPhase) {
        let (x, y) = match delta {
            winit::event::MouseScrollDelta::LineDelta(x, y) => (x as f64 * 30.0, y as f64 * 30.0),
            winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x, pos.y),
        };

        self.ctx.mouse_mut().accumulated_scroll.x += x;
        self.ctx.mouse_mut().accumulated_scroll.y += y;

        let height = self.ctx.size_info().cell_height() as f64;
        let lines = (self.ctx.mouse().accumulated_scroll.y / height) as i32;

        if lines != 0 {
            self.ctx.mouse_mut().accumulated_scroll.y -= lines as f64 * height;
            self.ctx.scroll(Scroll::Delta(lines));
        }
    }
}
