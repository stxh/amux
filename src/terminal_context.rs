use std::borrow::Cow;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::{Term, ClipboardType};
use alacritty_terminal::grid::Scroll;
use winit::event::Modifiers;

use crate::pane::Pane;
use crate::alacritty::input::ActionContext;
use crate::alacritty::event::{Mouse, TouchPurpose, InlineSearchState};
use crate::alacritty::display::{Display, SizeInfo};
use crate::alacritty::config::UiConfig;
use crate::alacritty::clipboard::Clipboard;
use crate::alacritty::scheduler::Scheduler;
use crate::terminal::TerminalEventListener;

pub struct TerminalPaneContext<'a> {
    pub pane: &'a mut Pane,
    pub terminal: &'a mut Term<TerminalEventListener>,
    pub config: &'a UiConfig,
    pub clipboard: &'a mut Clipboard,
    pub scheduler: &'a mut Scheduler,
    pub display: &'a mut Display,
}

impl<'a> ActionContext<TerminalEventListener> for TerminalPaneContext<'a> {
    fn write_to_pty<B: Into<Cow<'static, [u8]>>>(&self, data: B) {
        let bytes = data.into();
        let _ = self.pane.terminal.write_input(&bytes);
    }

    fn mark_dirty(&mut self) {
        // Placeholder
    }

    fn size_info(&self) -> SizeInfo {
        self.display.size_info
    }

    fn copy_selection(&mut self, ty: ClipboardType) {
        if let Some(text) = self.terminal.selection_to_string() {
            self.clipboard.store(ty, text);
        }
    }

    fn start_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        self.terminal.selection = Some(alacritty_terminal::selection::Selection::new(ty, point, side));
    }

    fn toggle_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        match &mut self.terminal.selection {
            Some(selection) if selection.ty == ty && !selection.is_empty() => {
                self.terminal.selection = None;
            },
            _ => {
                self.terminal.selection = Some(alacritty_terminal::selection::Selection::new(ty, point, side));
            }
        }
    }

    fn update_selection(&mut self, point: Point, side: Side) {
        if let Some(selection) = &mut self.terminal.selection {
            selection.update(point, side);
        }
    }

    fn clear_selection(&mut self) {
        self.terminal.selection = None;
    }

    fn selection_is_empty(&self) -> bool {
        self.terminal.selection.as_ref().map_or(true, |s| s.is_empty())
    }

    fn mouse_mut(&mut self) -> &mut Mouse {
        &mut self.pane.mouse
    }

    fn mouse(&self) -> &Mouse {
        &self.pane.mouse
    }

    fn touch_purpose(&mut self) -> &mut TouchPurpose {
        &mut self.pane.touch_purpose
    }

    fn modifiers(&mut self) -> &mut Modifiers {
        &mut self.pane.modifiers
    }

    fn scroll(&mut self, scroll: Scroll) {
        self.terminal.scroll_display(scroll);
    }

    fn display(&mut self) -> &mut Display {
        self.display
    }

    fn terminal(&self) -> &Term<TerminalEventListener> {
        self.terminal
    }

    fn terminal_mut(&mut self) -> &mut Term<TerminalEventListener> {
        self.terminal
    }

    fn config(&self) -> &UiConfig {
        self.config
    }

    fn clipboard_mut(&mut self) -> &mut Clipboard {
        self.clipboard
    }

    fn scheduler_mut(&mut self) -> &mut Scheduler {
        self.scheduler
    }

    fn search_active(&self) -> bool {
        self.pane.search_state.history_index.is_some()
    }

    fn inline_search_state(&mut self) -> &mut InlineSearchState {
        &mut self.pane.inline_search_state
    }
}
