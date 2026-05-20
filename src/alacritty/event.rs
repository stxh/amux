use std::time::{Instant, Duration};
use std::collections::VecDeque;
use winit::event::{MouseButton, ElementState};
use alacritty_terminal::index::{Point, Direction, Column, Side, Line};
use alacritty_terminal::term::search::Match;
use alacritty_terminal::grid::Scroll;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickState {
    None,
    Click,
    DoubleClick,
    TripleClick,
}

#[derive(Debug, Clone)]
pub struct Mouse {
    pub left_button_state: ElementState,
    pub middle_button_state: ElementState,
    pub right_button_state: ElementState,
    pub last_click_timestamp: Instant,
    pub last_click_button: MouseButton,
    pub click_state: ClickState,
    pub x: usize,
    pub y: usize,
    pub inside_text_area: bool,
    pub cell_side: Side,
    pub hint_highlight_dirty: bool,
    pub block_hint_launcher: bool,
    pub accumulated_scroll: ScrollAccumulator,
}

#[derive(Debug, Clone, Default)]
pub struct ScrollAccumulator {
    pub x: f64,
    pub y: f64,
}

impl Default for Mouse {
    fn default() -> Self {
        Self {
            left_button_state: ElementState::Released,
            middle_button_state: ElementState::Released,
            right_button_state: ElementState::Released,
            last_click_timestamp: Instant::now(),
            last_click_button: MouseButton::Left,
            click_state: ClickState::None,
            x: 0,
            y: 0,
            inside_text_area: false,
            cell_side: Side::Left,
            hint_highlight_dirty: false,
            block_hint_launcher: false,
            accumulated_scroll: Default::default(),
        }
    }
}

impl Mouse {
    pub fn point(&self, size_info: &crate::alacritty::display::SizeInfo, display_offset: usize) -> Point {
        // Simple implementation for now, will refine later
        let col = (self.x as f32 / size_info.cell_width()).floor() as usize;
        let line = (self.y as f32 / size_info.cell_height()).floor() as i32 - display_offset as i32;
        Point::new(Line(line), Column(col))
    }
}

pub struct SearchState {
    pub direction: Direction,
    pub history_index: Option<usize>,
    pub display_offset_delta: i32,
    pub origin: Point,
    pub focused_match: Option<Match>,
    pub history: VecDeque<String>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            direction: Direction::Right,
            history_index: None,
            display_offset_delta: 0,
            origin: Point::new(Line(0), Column(0)),
            focused_match: None,
            history: VecDeque::new(),
        }
    }
}

pub struct InlineSearchState {
    pub char_pending: bool,
    pub character: Option<char>,
    pub direction: Direction,
    pub stop_short: bool,
}

impl Default for InlineSearchState {
    fn default() -> Self {
        Self {
            char_pending: false,
            character: None,
            direction: Direction::Right,
            stop_short: false,
        }
    }
}

#[derive(Default)]
pub struct TouchPurpose;

pub enum EventType {
    Terminal(alacritty_terminal::event::Event),
    // ... other event types
}
