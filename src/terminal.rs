use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::grid::Grid;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::Processor;
use portable_pty::{Child, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

/// Terminal configuration
#[derive(Clone)]
pub struct TerminalConfig {
    pub rows: u16,
    pub cols: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            rows: 30,
            cols: 80,
            cell_width: 10,
            cell_height: 28,
        }
    }
}

/// Terminal size info implementing Dimensions trait
struct TermDimensions {
    columns: u16,
    lines: u16,
}

impl alacritty_terminal::grid::Dimensions for TermDimensions {
    fn columns(&self) -> usize {
        self.columns as usize
    }

    fn total_lines(&self) -> usize {
        self.lines as usize
    }

    fn screen_lines(&self) -> usize {
        self.lines as usize
    }
}

/// Wraps alacritty_terminal's Term with PTY I/O
pub struct Terminal {
    term: Arc<Mutex<Term<TerminalEventListener>>>,
    pty_master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send>>,
    pty_reader: Option<Box<dyn Read + Send>>,
    pty_writer: Option<Box<dyn Write + Send>>,
    config: TerminalConfig,
    event_receiver: mpsc::Receiver<alacritty_terminal::event::Event>,
    needs_redraw: Arc<Mutex<bool>>,
    pub redraw_notifier: Arc<Mutex<Option<xilem::core::MessageProxy<()>>>>,
    pub title: Arc<Mutex<Option<String>>>,
}

/// Event listener for terminal state changes
pub struct TerminalEventListener {
    sender: Option<mpsc::Sender<alacritty_terminal::event::Event>>,
    title: Arc<Mutex<Option<String>>>,
    redraw_notifier: Arc<Mutex<Option<xilem::core::MessageProxy<()>>>>,
}

impl alacritty_terminal::event::EventListener for TerminalEventListener {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        match &event {
            alacritty_terminal::event::Event::Title(t) => {
                if let Ok(mut title) = self.title.lock() {
                    *title = Some(t.clone());
                }
                if let Some(proxy) = &*self.redraw_notifier.lock().unwrap() {
                    let _ = proxy.message(());
                }
            }
            alacritty_terminal::event::Event::ResetTitle => {
                if let Ok(mut title) = self.title.lock() {
                    *title = None;
                }
                if let Some(proxy) = &*self.redraw_notifier.lock().unwrap() {
                    let _ = proxy.message(());
                }
            }
            _ => {}
        }
        if let Some(sender) = &self.sender {
            let _ = sender.send(event);
        }
    }
}

impl Terminal {
    pub fn new(config: &TerminalConfig) -> Self {
        let dimensions = TermDimensions {
            columns: config.cols,
            lines: config.rows,
        };

        let (event_sender, event_receiver) = mpsc::channel();
        let needs_redraw = Arc::new(Mutex::new(false));
        let title = Arc::new(Mutex::new(None));
        let redraw_notifier = Arc::new(Mutex::new(None));

        let listener = TerminalEventListener {
            sender: Some(event_sender),
            title: title.clone(),
            redraw_notifier: redraw_notifier.clone(),
        };

        let term = Term::new(
            alacritty_terminal::term::Config::default(),
            &dimensions,
            listener,
        );

        Self {
            term: Arc::new(Mutex::new(term)),
            pty_master: None,
            child: None,
            pty_reader: None,
            pty_writer: None,
            config: config.clone(),
            event_receiver,
            needs_redraw,
            redraw_notifier,
            title,
        }
    }

    /// Start a PTY shell process (e.g., zsh, bash, powershell)
    pub fn spawn_shell(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let pty_system = portable_pty::native_pty_system();

        let size = PtySize {
            rows: self.config.rows,
            cols: self.config.cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(size)?;
        let cmd = portable_pty::CommandBuilder::new_default_prog();

        self.child = Some(pair.slave.spawn_command(cmd)?);
        self.pty_master = Some(pair.master);

        // Split reader/writer for async I/O
        if let Some(master) = &mut self.pty_master {
            let reader = master.try_clone_reader()?;
            let writer = master.take_writer()?;
            self.pty_reader = Some(reader);
            self.pty_writer = Some(writer);
        }

        // Start background thread to read PTY output and feed to terminal
        let term_clone = self.term.clone();
        let needs_redraw = self.needs_redraw.clone();
        let reader = self.pty_reader.take();
        let redraw_notifier = self.redraw_notifier.clone();

        thread::spawn(move || {
            // println!("DEBUG: PTY reader thread started!");
            if let Some(mut reader) = reader {
                let mut buf = [0u8; 4096];
                // Create VTE parser - this parses ANSI escape sequences and calls
                // the appropriate methods on the terminal (input, execute, etc.)
                let mut parser: Processor<alacritty_terminal::vte::ansi::StdSyncHandler> = Processor::new();
                let mut osc_parser = OscNotificationParser::new();

                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            // println!("DEBUG: PTY reader thread read 0 bytes (EOF). Exiting loop.");
                            break;
                        }
                        Err(_e) => {
                            break;
                        }
                        Ok(bytes_read) => {
                            // Process bytes through OSC parser for notifications
                            for byte in &buf[..bytes_read] {
                                osc_parser.advance(*byte);
                            }

                            // Parse PTY bytes through VTE parser
                            // This correctly handles ANSI escape sequences
                            if let Ok(mut term) = term_clone.lock() {
                                for byte in &buf[..bytes_read] {
                                    parser.advance(&mut *term, *byte);
                                }
                            }
                            // Coalesce redraw trigger: only send message if needs_redraw was false
                            let already_pending = {
                                let mut needs = needs_redraw.lock().unwrap();
                                let prev = *needs;
                                *needs = true;
                                prev
                            };
                            if !already_pending {
                                if let Some(proxy) = &*redraw_notifier.lock().unwrap() {
                                    let _ = proxy.message(());
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Write input to the PTY (user keystrokes)
    pub fn write_input(&mut self, data: &[u8]) -> std::io::Result<()> {
        if let Some(writer) = &mut self.pty_writer {
            writer.write_all(data)?;
            writer.flush()?;
        }
        Ok(())
    }

    /// Check if terminal needs redraw and consume the flag
    pub fn check_and_clear_redraw(&self) -> bool {
        let mut needs = self.needs_redraw.lock().unwrap();
        let val = *needs;
        *needs = false;
        val
    }

    /// Get the active terminal shell title
    pub fn title(&self) -> Option<String> {
        self.title.lock().unwrap().clone()
    }

    /// Get a copy of the terminal grid for rendering
    pub fn grid(&self) -> Grid<Cell> {
        let term = self.term.lock().unwrap();
        term.grid().clone()
    }

    /// Get the number of columns in the grid
    pub fn grid_cols(&self) -> usize {
        let term = self.term.lock().unwrap();
        term.grid().columns()
    }

    /// Get the number of rows in the grid
    pub fn grid_rows(&self) -> usize {
        let term = self.term.lock().unwrap();
        term.grid().screen_lines()
    }

    /// Get the config
    pub fn config(&self) -> &TerminalConfig {
        &self.config
    }

    /// Resize the terminal
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.config.cols = cols;
        self.config.rows = rows;

        let dimensions = TermDimensions {
            columns: cols,
            lines: rows,
        };
        let mut term = self.term.lock().unwrap();
        term.resize(dimensions);
        drop(term);

        if let Some(master) = &mut self.pty_master {
            let size = portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            };
            let _ = master.resize(size);
        }
    }

    /// Get the display offset (for scrolling)
    pub fn display_offset(&self) -> usize {
        let term = self.term.lock().unwrap();
        term.grid().display_offset()
    }

    /// Get cursor position
    pub fn cursor(&self) -> Point {
        let term = self.term.lock().unwrap();
        let grid = term.grid();
        grid.cursor.point
    }

    /// Check if cell at (line, col) is in selection
    pub fn is_cell_selected(&self, line: i32, col: usize) -> bool {
        let term = self.term.lock().unwrap();
        if let Some(selection) = &term.selection {
            use alacritty_terminal::index::{Point, Column, Line};
            if let Some(range) = selection.to_range(&*term) {
                range.contains(Point::new(Line(line), Column(col)))
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Clear selection
    pub fn clear_selection(&self) {
        let mut term = self.term.lock().unwrap();
        term.selection = None;
    }

    /// Start a new selection with the given type
    pub fn start_selection_with_type(&self, line: i32, col: usize, ty: alacritty_terminal::selection::SelectionType) {
        use alacritty_terminal::index::{Point, Column, Line, Side};
        use alacritty_terminal::selection::Selection;
        let mut term = self.term.lock().unwrap();
        let point = Point::new(Line(line), Column(col));
        term.selection = Some(Selection::new(ty, point, Side::Left));
    }

    /// Start a new simple (character-based) selection
    pub fn start_selection(&self, line: i32, col: usize) {
        use alacritty_terminal::selection::SelectionType;
        self.start_selection_with_type(line, col, SelectionType::Simple);
    }

    /// Update selection end point
    pub fn update_selection(&self, line: i32, col: usize) {
        use alacritty_terminal::index::{Point, Column, Line, Side};
        let mut term = self.term.lock().unwrap();
        if let Some(selection) = &mut term.selection {
            let point = Point::new(Line(line), Column(col));
            selection.update(point, Side::Right);
        }
    }

    /// Convert selection to string
    pub fn selection_to_string(&self) -> Option<String> {
        let term = self.term.lock().unwrap();
        term.selection_to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    Ground,
    Esc,
    Osc,
    OscEsc,
}

struct OscNotificationParser {
    state: ParserState,
    buffer: Vec<u8>,
}

impl OscNotificationParser {
    fn new() -> Self {
        Self {
            state: ParserState::Ground,
            buffer: Vec::new(),
        }
    }

    fn advance(&mut self, byte: u8) {
        match self.state {
            ParserState::Ground => {
                if byte == 0x1b {
                    self.state = ParserState::Esc;
                } else if byte == 0x9d {
                    self.buffer.clear();
                    self.state = ParserState::Osc;
                }
            }
            ParserState::Esc => {
                if byte == 0x5d {
                    self.buffer.clear();
                    self.state = ParserState::Osc;
                } else {
                    self.state = ParserState::Ground;
                }
            }
            ParserState::Osc => {
                if byte == 0x07 {
                    self.handle_osc();
                    self.state = ParserState::Ground;
                } else if byte == 0x1b {
                    self.state = ParserState::OscEsc;
                } else {
                    self.buffer.push(byte);
                }
            }
            ParserState::OscEsc => {
                if byte == 0x5c {
                    self.handle_osc();
                    self.state = ParserState::Ground;
                } else {
                    self.buffer.push(0x1b);
                    self.buffer.push(byte);
                    self.state = ParserState::Osc;
                }
            }
        }
    }

    fn handle_osc(&mut self) {
        let payload_str = match std::str::from_utf8(&self.buffer) {
            Ok(s) => s,
            Err(_) => return,
        };

        let mut title = String::new();
        let mut body = String::new();
        let mut triggered = false;

        if payload_str.starts_with("9;") {
            title = "amux".to_string();
            body = payload_str["9;".len()..].to_string();
            triggered = true;
        } else if payload_str.starts_with("777;notify;") {
            let payload = &payload_str["777;notify;".len()..];
            let parts: Vec<&str> = payload.split(';').collect();
            if parts.len() >= 2 {
                title = parts[0].trim().to_string();
                body = parts[1].trim().to_string();
            } else if parts.len() == 1 {
                title = "amux".to_string();
                body = parts[0].trim().to_string();
            }
            triggered = true;
        } else if payload_str.starts_with("99;") {
            let payload = &payload_str["99;".len()..];
            let parts: Vec<&str> = payload.split(';').collect();
            for part in parts {
                let part = part.trim();
                // Skip options like i=, d=, p=, a=
                if part.contains('=') && (part.starts_with("i=") || part.starts_with("d=") || part.starts_with("p=") || part.starts_with("a=")) {
                    continue;
                }
                if title.is_empty() {
                    title = part.to_string();
                } else if body.is_empty() {
                    body = part.to_string();
                } else {
                    body.push_str("; ");
                    body.push_str(part);
                }
            }
            if body.is_empty() && !title.is_empty() {
                body = title;
                title = "amux".to_string();
            }
            triggered = true;
        }

        if triggered && !body.is_empty() {
            let title = if title.is_empty() { "amux".to_string() } else { title };
            std::thread::spawn(move || {
                let _ = notify_rust::Notification::new()
                    .summary(&title)
                    .body(&body)
                    .appname("amux")
                    .show();
            });
        }
    }
}

