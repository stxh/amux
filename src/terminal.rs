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
}

/// Event listener for terminal state changes
pub struct TerminalEventListener {
    sender: Option<mpsc::Sender<alacritty_terminal::event::Event>>,
}

impl alacritty_terminal::event::EventListener for TerminalEventListener {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
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

        let listener = TerminalEventListener {
            sender: Some(event_sender),
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

        thread::spawn(move || {
            if let Some(mut reader) = reader {
                let mut buf = [0u8; 4096];
                // Create VTE parser - this parses ANSI escape sequences and calls
                // the appropriate methods on the terminal (input, execute, etc.)
                let mut parser: Processor<alacritty_terminal::vte::ansi::StdSyncHandler> = Processor::new();

                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(bytes_read) => {
                            // Parse PTY bytes through VTE parser
                            // This correctly handles ANSI escape sequences
                            if let Ok(mut term) = term_clone.lock() {
                                for byte in &buf[..bytes_read] {
                                    parser.advance(&mut *term, *byte);
                                }
                            }
                            *needs_redraw.lock().unwrap() = true;
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
}
