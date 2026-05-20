use alacritty_terminal::term::TermMode;

pub struct UiConfig {
    pub scrolling: Scrolling,
    pub selection: Selection,
    pub debug: Debug,
    // Add other fields as needed
}

pub struct Scrolling {
    pub multiplier: u8,
}

pub struct Selection {
    pub save_to_clipboard: bool,
}

pub struct Debug {
    pub log_level: log::LevelFilter,
    pub print_events: bool,
    pub ref_test: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            scrolling: Scrolling { multiplier: 3 },
            selection: Selection { save_to_clipboard: true },
            debug: Debug {
                log_level: log::LevelFilter::Info,
                print_events: false,
                ref_test: false,
            },
        }
    }
}

pub enum Action {
    Paste,
    Copy,
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ScrollPageUp,
    ScrollPageDown,
    ScrollLineUp,
    ScrollLineDown,
    ScrollToTop,
    ScrollToBottom,
    ClearHistory,
    ToggleViMode,
    SearchForward,
    SearchBackward,
    Quit,
    // ... others
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingMode(u32);

impl BindingMode {
    pub fn new(mode: &TermMode, search: bool) -> Self {
        // Simplified
        Self(0)
    }
}
