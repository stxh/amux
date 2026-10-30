use libghostty_vt::key::Mods;
use libghostty_vt::mouse::{
    Action as MouseAction, Button as MouseButton, Encoder as MouseEncoder, EncoderSize,
    Event as MouseEvent, Position as MousePosition,
};
use libghostty_vt::render::{CellIterator, RenderState, RowIterator};
use libghostty_vt::selection::{FormatOptions, SelectLineOptions, SelectWordOptions, Selection};
use libghostty_vt::style::{Palette, RgbColor};
use libghostty_vt::terminal::{Options as TerminalOptions, Point, PointCoordinate, ScrollViewport};
use libghostty_vt::Terminal as GhosttyTerminal;
use portable_pty::{MasterPty, PtySize};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Initial working directory for spawned shells (from `amux <path>`).
    pub working_dir: Option<std::path::PathBuf>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            rows: 30,
            cols: 80,
            cell_width: 10,
            cell_height: 28,
            working_dir: None,
        }
    }
}

const MAX_SCROLLBACK: usize = 10000;

/// Catppuccin Mocha 256-color palette injected into the engine so it is
/// the single source of truth for colors (OSC 4/10/11 then work naturally).
fn catppuccin_palette() -> Palette {
    let ansi16: [(u8, u8, u8); 16] = [
        (0x45, 0x47, 0x5a), (0xf3, 0x8b, 0xa8), (0xa6, 0xe3, 0xa1), (0xf9, 0xe2, 0xaf),
        (0x89, 0xb4, 0xfa), (0xcb, 0xb6, 0xff), (0x94, 0xe2, 0xd5), (0xc6, 0xd0, 0xf5),
        (0x58, 0x5b, 0x70), (0xf3, 0x8b, 0xa8), (0xa6, 0xe3, 0xa1), (0xf9, 0xe2, 0xaf),
        (0x89, 0xb4, 0xfa), (0xcb, 0xb6, 0xff), (0x94, 0xe2, 0xd5), (0xc6, 0xd0, 0xf5),
    ];
    let mut entries = [RgbColor { r: 0, g: 0, b: 0 }; 256];
    for (i, c) in ansi16.iter().enumerate() {
        entries[i] = RgbColor { r: c.0, g: c.1, b: c.2 };
    }
    for i in 0..216u32 {
        entries[16 + i as usize] = RgbColor {
            r: ((i / 36) * 51) as u8,
            g: (((i % 36) / 6) * 51) as u8,
            b: ((i % 6) * 51) as u8,
        };
    }
    for i in 0..24u32 {
        let gray = (8 + i * 10) as u8;
        entries[232 + i as usize] = RgbColor { r: gray, g: gray, b: gray };
    }
    Palette(entries)
}

/// Ghostty engine + render state, owned by the UI thread.
pub(crate) struct Engine {
    pub term: GhosttyTerminal<'static, 'static>,
    pub render: RenderState<'static>,
    pub rows: RowIterator<'static>,
    pub cells: CellIterator<'static>,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Closing a pane must terminate its shell, or the process leaks.
        if let Some(killer) = &mut self.child_killer {
            let _ = killer.kill();
        }
    }
}

/// Wraps libghostty-vt's Terminal with PTY I/O.
///
/// The Ghostty terminal is `!Send`, so it is owned by the UI thread inside a
/// `RefCell`. The PTY reader thread only pushes raw byte chunks through a
/// channel; the UI thread feeds them via [`Terminal::check_and_clear_redraw`].
pub struct Terminal {
    engine: RefCell<Engine>,
    pty_master: Option<Box<dyn MasterPty + Send>>,
    child_killer: Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
    pty_writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    config: TerminalConfig,
    needs_redraw: Arc<Mutex<bool>>,
    shell_exited: Arc<AtomicBool>,
    shell_aborted: Arc<Mutex<Option<u32>>>,
    pub redraw_notifier: Arc<Mutex<Option<xilem::core::MessageProxy<()>>>>,
    pub title: Arc<Mutex<Option<String>>>,
    /// Working directory reported by the shell via OSC 7, if it does that.
    osc_pwd: Arc<Mutex<Option<String>>>,
    /// Live shell process id, used to read its current directory.
    shell_pid: Option<u32>,
    sel_anchor: Option<Point>,
}

impl Terminal {
    pub fn new(config: &TerminalConfig) -> Self {
        let needs_redraw = Arc::new(Mutex::new(false));
        let shell_exited = Arc::new(AtomicBool::new(false));
        let shell_aborted = Arc::new(Mutex::new(None));
        let title = Arc::new(Mutex::new(None));
        let osc_pwd: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let redraw_notifier: Arc<Mutex<Option<xilem::core::MessageProxy<()>>>> =
            Arc::new(Mutex::new(None));
        let pty_writer: Arc<Mutex<Option<Box<dyn Write + Send>>>> = Arc::new(Mutex::new(None));
        let (_, rx) = mpsc::channel::<Vec<u8>>();

        let mut term = GhosttyTerminal::new(TerminalOptions {
            cols: config.cols,
            rows: config.rows,
            max_scrollback: MAX_SCROLLBACK,
        })
        .expect("failed to create ghostty terminal");

        term.set_default_fg_color(Some(RgbColor { r: 0xc6, g: 0xd0, b: 0xf5 }))
            .expect("set default fg");
        term.set_default_bg_color(Some(RgbColor { r: 0x1e, g: 0x1e, b: 0x2e }))
            .expect("set default bg");
        term.set_default_color_palette(Some(catppuccin_palette()))
            .expect("set palette");

        // Route query responses (DSR/DA/cursor reports) back to the PTY.
        {
            let writer = pty_writer.clone();
            term.on_pty_write(move |_t: &GhosttyTerminal, data: &[u8]| {
                if let Ok(mut w) = writer.lock() {
                    if let Some(w) = w.as_mut() {
                        let _ = w.write_all(data);
                        let _ = w.flush();
                    }
                }
            })
            .expect("register pty write handler");
        }

        // Mirror OSC title changes into a shared slot and wake the UI.
        {
            let title = title.clone();
            let notifier = redraw_notifier.clone();
            term.on_title_changed(move |t: &GhosttyTerminal| {
                let new = t
                    .title()
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_owned());
                let changed = match title.lock() {
                    Ok(mut cur) => {
                        if cur.as_deref() == new.as_deref() {
                            false
                        } else {
                            *cur = new;
                            true
                        }
                    }
                    Err(_) => false,
                };
                // Only a real change is worth a wake-up: the message re-records
                // and re-renders the whole grid, and shells repaint their title
                // (and send OSC 0) far more often than they actually change it.
                if changed {
                    if let Some(proxy) = &*notifier.lock().unwrap() {
                        let _ = proxy.message(());
                    }
                }
            })
            .expect("register title handler");
        }

        // Mirror OSC 7 working-directory reports. Stock Windows shells do not
        // send them, but bash/zsh (and PowerShell with shell integration) do,
        // and it costs one cached string to honour them.
        {
            let pwd = osc_pwd.clone();
            term.on_pwd_changed(move |t: &GhosttyTerminal| {
                let new = t.pwd().ok().filter(|s| !s.is_empty()).map(|s| s.to_owned());
                if let Ok(mut cur) = pwd.lock() {
                    *cur = new;
                }
            })
            .expect("register pwd handler");
        }

        let render = RenderState::new().expect("failed to create render state");
        let rows = RowIterator::new().expect("failed to create row iterator");
        let cells = CellIterator::new().expect("failed to create cell iterator");

        Self {
            engine: RefCell::new(Engine { term, render, rows, cells }),
            pty_master: None,
            child_killer: None,
            pty_writer,
            pty_rx: Arc::new(Mutex::new(rx)),
            config: config.clone(),
            needs_redraw,
            shell_exited,
            shell_aborted,
            redraw_notifier,
            title,
            osc_pwd,
            shell_pid: None,
            sel_anchor: None,
        }
    }

    /// Borrow the engine mutably (UI-thread only).
    pub(crate) fn engine(&self) -> std::cell::RefMut<'_, Engine> {
        self.engine.borrow_mut()
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
        let mut cmd = portable_pty::CommandBuilder::new_default_prog();
        if let Some(dir) = &self.config.working_dir {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd)?;
        // `cd` moves the shell process' own directory, so remember its pid to
        // read the live path back for the activity-bar tooltip.
        self.shell_pid = child.process_id();
        self.child_killer = Some(child.clone_killer());
        self.pty_master = Some(pair.master);

        // On Windows the ConPTY reader never sees EOF when the shell exits,
        // so a dedicated thread waits on the child and flags termination.
        // Only a clean exit (status 0, e.g. the user typed `exit`) closes the
        // pane. A shell killed by a child TUI (e.g. opencode's parent-kill
        // bug) is an abnormal exit: keep the window alive and notify instead.
        {
            let shell_exited = self.shell_exited.clone();
            let shell_aborted = self.shell_aborted.clone();
            let redraw_notifier = self.redraw_notifier.clone();
            thread::spawn(move || {
                let mut child = child;
                match child.wait() {
                    Ok(status) if status.success() => {
                        shell_exited.store(true, Ordering::Relaxed);
                    }
                    Ok(status) => {
                        if let Ok(mut c) = shell_aborted.lock() {
                            *c = Some(status.exit_code());
                        }
                    }
                    Err(_) => {}
                }
                if let Some(proxy) = &*redraw_notifier.lock().unwrap() {
                    let _ = proxy.message(());
                }
            });
        }

        // Split reader/writer for async I/O
        let reader;
        {
            let master = self.pty_master.as_mut().expect("pty master");
            reader = master.try_clone_reader()?;
            let writer = master.take_writer()?;
            *self.pty_writer.lock().unwrap() = Some(writer);
        }

        // Background thread: read PTY output, sniff OSC notifications, and push
        // raw chunks to the UI thread which feeds them to the terminal.
        let needs_redraw = self.needs_redraw.clone();
        let redraw_notifier = self.redraw_notifier.clone();
        let pty_writer = self.pty_writer.clone();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        self.pty_rx = Arc::new(Mutex::new(rx));

        thread::spawn(move || {
            let mut reader = reader;
            // Bigger than a pipe's usual granularity: `cat`ing a big file used
            // to wake the UI once per 4 KiB chunk, and every wake costs a
            // channel push plus a repaint of the whole grid.
            let mut buf = [0u8; 65536];
            let mut osc_parser = OscNotificationParser::new();

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Err(_) => break,
                    Ok(bytes_read) => {
                        for byte in &buf[..bytes_read] {
                            osc_parser.advance(*byte);
                        }

                        if tx.send(buf[..bytes_read].to_vec()).is_err() {
                            break;
                        }

                        // Coalesce redraw trigger: only send message if none pending
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

            // PTY read broke. This is NOT proof the shell exited: ConPTY can
            // transiently break its output pipe (e.g. when a full-screen TUI
            // resets the console on exit) while the shell is still alive.
            // Only the child.wait() thread may flag shell exit. Drop the
            // writer so any pending query callbacks stop, and wake the UI.
            *pty_writer.lock().unwrap() = None;
            if let Some(proxy) = &*redraw_notifier.lock().unwrap() {
                let _ = proxy.message(());
            }
        });

        Ok(())
    }

    /// Write input to the PTY (user keystrokes)
    pub fn write_input(&mut self, data: &[u8]) -> std::io::Result<()> {
        // Typing always returns the view to the live prompt.
        self.scroll_to_bottom();
        if let Some(writer) = &mut *self.pty_writer.lock().unwrap() {
            writer.write_all(data)?;
            writer.flush()?;
        }
        Ok(())
    }

    /// Drain pending PTY data into the terminal and report whether a redraw is needed
    pub fn check_and_clear_redraw(&self) -> bool {
        let mut fed = false;
        {
            // One lock for the whole batch: output usually arrives in bursts,
            // and taking the channel lock per chunk showed up under heavy
            // output. No other thread takes this lock, so feeding the engine
            // while holding it cannot deadlock against the PTY writer.
            let rx = self.pty_rx.lock().unwrap();
            while let Ok(data) = rx.try_recv() {
                self.engine.borrow_mut().term.vt_write(&data);
                fed = true;
            }
        }

        let mut needs = self.needs_redraw.lock().unwrap();
        let val = *needs || fed;
        *needs = false;
        val
    }

    /// Whether the shell process has exited (PTY reader thread saw EOF).
    pub fn shell_exited(&self) -> bool {
        self.shell_exited.load(Ordering::Relaxed)
    }

    /// Take the abnormal shell exit code once (killed by a child TUI, etc.).
    /// The pane is kept alive so the user sees the message and closes it.
    pub fn take_shell_abort(&self) -> Option<u32> {
        let mut code = self.shell_aborted.lock().unwrap();
        code.take()
    }

    /// Append a notice line to the terminal grid (UI thread).
    pub fn print_notice(&self, text: &str) {
        let mut payload = String::from("\r\n");
        payload.push_str(text);
        self.engine.borrow_mut().term.vt_write(payload.as_bytes());
        *self.needs_redraw.lock().unwrap() = true;
    }

    /// Get the active terminal shell title
    pub fn title(&self) -> Option<String> {
        self.title.lock().unwrap().clone()
    }

    /// The shell's current working directory.
    ///
    /// Collected while the shell is alive, in order of authority:
    ///
    /// 1. OSC 7 — the shell said so itself (needs shell integration).
    /// 2. The PowerShell prompt on screen — PowerShell's `cd` never moves the
    ///    process directory, so this is its only live source.
    /// 3. The shell process' own directory — `cd` moves it for cmd.exe, bash
    ///    and friends.
    /// 4. The directory the shell was spawned in, for the moment before any of
    ///    the above is available.
    pub fn current_dir(&self) -> Option<String> {
        // A directory the shell reported itself is authoritative.
        if let Some(pwd) = self.osc_pwd.lock().ok().and_then(|pwd| pwd.clone()) {
            return Some(decode_file_url(&pwd));
        }
        if self.shell_exited() {
            return self.config.working_dir.as_ref().map(|p| p.display().to_string());
        }
        // PowerShell keeps its own location; only the prompt shows it.
        if let Some(dir) = self.powershell_dir() {
            return Some(dir);
        }
        // cmd.exe, bash and friends chdir themselves, so their live directory
        // can be read straight out of the process.
        if let Some(pid) = self.shell_pid {
            if let Some(dir) = crate::proc_cwd::process_cwd(pid) {
                if !dir.is_empty() {
                    return Some(dir);
                }
            }
        }
        self.config
            .working_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string()))
    }

    /// The directory a PowerShell prompt on screen reports, if the cursor sits
    /// on one.
    ///
    /// `Set-Location` only moves the process directory around native commands
    /// (it restores it afterwards), so a PowerShell session cannot be followed
    /// through the process; the default prompt still prints the location, and
    /// the cursor rests on that line.
    fn powershell_dir(&self) -> Option<String> {
        let line = self.cursor_line()?;
        let path = powershell_prompt_path(&line)?;
        // A prompt may name a non-filesystem location (`HKLM:\...`) or stale
        // text; only a real directory is worth showing.
        std::path::Path::new(path).is_dir().then(|| path.to_string())
    }

    /// The text of the row the cursor is on, control cells dropped.
    ///
    /// Only that one row is read, which is what keeps the prompt check cheap:
    /// the location has to be looked up on every repaint of the tooltip.
    fn cursor_line(&self) -> Option<String> {
        let mut guard = self.engine();
        // Reborrow the engine as a plain `&mut Engine` so the render, row and
        // cell iterators are borrowed as separate fields, like `render_grid`
        // does; going through `RefMut` for each of them would be one borrow of
        // the whole engine.
        let eng = &mut *guard;
        let snapshot = eng.render.update(&eng.term).ok()?;
        if !snapshot.cursor_visible().ok()? {
            return None;
        }
        let row_idx = snapshot.cursor_viewport().ok().flatten()?.y as usize;
        let mut row_iter = eng.rows.update(&snapshot).ok()?;
        let mut line = String::new();
        let mut glyph = String::new();
        for idx in 0..=row_idx {
            let row = row_iter.next()?;
            if idx < row_idx {
                continue;
            }
            if let Ok(mut cells) = eng.cells.update(row) {
                while let Some(cell) = cells.next() {
                    // The C API replaces the buffer, so it is reused per cell.
                    glyph.clear();
                    let _ = cell.graphemes_utf8(&mut glyph);
                    line.extend(glyph.chars().filter(|ch| !ch.is_control()));
                }
            }
            break;
        }
        Some(line)
    }

    /// Get the number of columns in the grid
    pub fn grid_cols(&self) -> usize {
        self.engine.borrow().term.cols().unwrap_or(0) as usize
    }

    /// Get the number of rows in the grid
    pub fn grid_rows(&self) -> usize {
        self.engine.borrow().term.rows().unwrap_or(0) as usize
    }

    /// Where the caret sits, in viewport cells.
    ///
    /// A cursor resting on the tail of a wide character belongs to that
    /// character's first cell, matching where `render_grid` draws the cursor
    /// block; that is the anchor the IME candidate window should follow.
    pub fn cursor_cell(&self) -> Option<(u16, u16)> {
        let mut guard = self.engine();
        // Reborrow so the render state and the terminal stay separate fields,
        // like `cursor_line` does.
        let eng = &mut *guard;
        let snapshot = eng.render.update(&eng.term).ok()?;
        let pos = snapshot.cursor_viewport().ok().flatten()?;
        let col = if pos.at_wide_tail && pos.x > 0 {
            pos.x - 1
        } else {
            pos.x
        };
        Some((col, pos.y))
    }

    /// True when the hosted app has enabled mouse tracking (DEC 1000/1002/1003...).
    /// When true, mouse events should be reported to the PTY instead of used for
    /// local selection.
    pub fn mouse_is_reporting(&self) -> bool {
        self.engine.borrow().term.is_mouse_tracking().unwrap_or(false)
    }

    /// Encode a mouse event into the terminal's active report protocol and write it
    /// to the PTY. Returns true if any bytes were sent.
    ///
    /// - `btn_id`: 0=left, 1=right, 2=middle, 3=wheel-up, 4=wheel-down, 255=none.
    /// - `action`: 0=press, 1=release, 2=motion.
    /// - `x`/`y`: grid-relative logical pixel coords (origin at cell 0,0 top-left).
    pub fn mouse_report(
        &mut self,
        btn_id: u8,
        action: u8,
        any_button: bool,
        x: f32,
        y: f32,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> bool {
        let cell_w = self.config.cell_width.max(1) as u32;
        let cell_h = self.config.cell_height.max(1) as u32;
        let cols = self.grid_cols() as u32;
        let rows = self.grid_rows() as u32;

        let mut encoder = match MouseEncoder::new() {
            Ok(e) => e,
            Err(_) => return false,
        };
        {
            let engine = self.engine.borrow();
            encoder.set_options_from_terminal(&engine.term);
        }
        encoder.set_any_button_pressed(any_button);
        encoder.set_size(EncoderSize {
            screen_width: cols.saturating_mul(cell_w),
            screen_height: rows.saturating_mul(cell_h),
            cell_width: cell_w,
            cell_height: cell_h,
            padding_top: 0,
            padding_bottom: 0,
            padding_left: 0,
            padding_right: 0,
        });

        let button = match btn_id {
            0 => Some(MouseButton::Left),
            1 => Some(MouseButton::Right),
            2 => Some(MouseButton::Middle),
            3 => Some(MouseButton::Four),
            4 => Some(MouseButton::Five),
            _ => None,
        };
        let act = match action {
            1 => MouseAction::Release,
            2 => MouseAction::Motion,
            _ => MouseAction::Press,
        };
        let mut mods = Mods::empty();
        if shift {
            mods |= Mods::SHIFT;
        }
        if alt {
            mods |= Mods::ALT;
        }
        if ctrl {
            mods |= Mods::CTRL;
        }

        let mut event = match MouseEvent::new() {
            Ok(e) => e,
            Err(_) => return false,
        };
        event
            .set_action(act)
            .set_button(button)
            .set_mods(mods)
            .set_position(MousePosition { x, y });

        let mut out = Vec::new();
        if encoder.encode_to_vec(&event, &mut out).is_err() || out.is_empty() {
            return false;
        }
        let _ = self.write_input(&out);
        true
    }

    /// Resize the terminal including cell pixel dimensions
    pub fn resize_px(&mut self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        self.config.cols = cols;
        self.config.rows = rows;
        self.config.cell_width = cell_w;
        self.config.cell_height = cell_h;

        {
            let mut engine = self.engine.borrow_mut();
            let _ = engine.term.resize(cols, rows, cell_w as u32, cell_h as u32);
        }

        if let Some(master) = &mut self.pty_master {
            let size = portable_pty::PtySize {
                rows,
                cols,
                pixel_width: cell_w * cols,
                pixel_height: cell_h * rows,
            };
            let _ = master.resize(size);
        }
    }

    /// Scroll the viewport by delta_lines (positive = scroll up into history, negative = scroll down)
    pub fn scroll_viewport(&mut self, delta_lines: i32) {
        let mut engine = self.engine.borrow_mut();
        engine
            .term
            .scroll_viewport(ScrollViewport::Delta(-(delta_lines as isize)));
    }

    /// Jump to the active area (bottom of scrollback)
    pub fn scroll_to_bottom(&mut self) {
        let mut engine = self.engine.borrow_mut();
        engine.term.scroll_viewport(ScrollViewport::Bottom);
    }

    /// Scrollbar state: (total rows, viewport offset, visible rows).
    ///
    /// `offset` grows as the viewport scrolls up into history. Used to map
    /// scrollbar drags onto scroll positions.
    pub fn scrollbar_state(&self) -> (usize, usize, usize) {
        let engine = self.engine.borrow();
        match engine.term.scrollbar() {
            Ok(s) => (s.total as usize, s.offset as usize, s.len as usize),
            Err(_) => (0, 0, 0),
        }
    }

    fn viewport_point(&self, line: i32, col: usize) -> Point {
        Point::Viewport(PointCoordinate {
            x: col as u16,
            y: line.max(0) as u32,
        })
    }

    /// Anchor point in screen space, where row 0 is the top of the scrollback.
    ///
    /// Viewport coordinates describe a *visible* row, so an anchor kept in that
    /// space drifts with the content while scrolling (the selection then slides
    /// instead of growing). Screen space is stable, which is what a selection
    /// anchor needs during scrolling and edge auto-scroll.
    fn screen_point(&self, line: i32, col: usize) -> Point {
        let engine = self.engine.borrow();
        let offset = engine.term.scrollbar().map(|s| s.offset).unwrap_or(0) as u32;
        Point::Screen(PointCoordinate {
            x: col as u16,
            y: offset.saturating_add(line.max(0) as u32),
        })
    }

    /// Start a new character-based selection at the given viewport position
    pub fn start_selection(&mut self, line: i32, col: usize) {
        let anchor = self.screen_point(line, col);
        self.sel_anchor = Some(anchor);
        let engine = self.engine.borrow();
        if let (Ok(a), Ok(b)) = (engine.term.grid_ref(anchor), engine.term.grid_ref(anchor)) {
            let sel = Selection::new(a, b, false);
            let _ = engine.term.set_selection(Some(&sel));
        }
    }

    /// Start a word selection (double-click) at the given viewport position
    pub fn start_word_selection(&mut self, line: i32, col: usize) {
        let anchor = self.screen_point(line, col);
        self.sel_anchor = Some(anchor);
        let engine = self.engine.borrow();
        if let Ok(grid_ref) = engine.term.grid_ref(anchor) {
            let sel = engine
                .term
                .select_word(SelectWordOptions::new(grid_ref))
                .ok()
                .flatten();
            if let Some(sel) = sel {
                let _ = engine.term.set_selection(Some(&sel));
            }
        }
    }

    /// Start a line selection (triple-click) at the given viewport position
    pub fn start_line_selection(&mut self, line: i32, col: usize) {
        let anchor = self.screen_point(line, col);
        self.sel_anchor = Some(anchor);
        let engine = self.engine.borrow();
        if let Ok(grid_ref) = engine.term.grid_ref(anchor) {
            let sel = engine
                .term
                .select_line(SelectLineOptions::new(grid_ref))
                .ok()
                .flatten();
            if let Some(sel) = sel {
                let _ = engine.term.set_selection(Some(&sel));
            }
        }
    }

    /// Extend the current selection to the given viewport position
    pub fn update_selection(&mut self, line: i32, col: usize) {
        let Some(anchor) = self.sel_anchor else { return };
        let focus = self.viewport_point(line, col);
        let engine = self.engine.borrow();
        if let (Ok(a), Ok(b)) = (engine.term.grid_ref(anchor), engine.term.grid_ref(focus)) {
            let sel = Selection::new(a, b, false);
            let _ = engine.term.set_selection(Some(&sel));
        }
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.sel_anchor = None;
        let engine = self.engine.borrow();
        let _ = engine.term.set_selection(None);
    }

    /// Convert selection to string
    pub fn selection_to_string(&self) -> Option<String> {
        let engine = self.engine.borrow();
        let bytes = engine
            .term
            .format_selection_alloc(None, FormatOptions::new())
            .ok()
            .flatten()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
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

/// Directory named by a PowerShell prompt line (`PS C:\dir> ...`).
///
/// Only the default prompt shape is recognised: a custom prompt (oh-my-posh,
/// starship) does not print the location, and those sessions need OSC 7 shell
/// integration instead.
fn powershell_prompt_path(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("PS ")?;
    let path = rest.split_once('>')?.0.trim();
    let mut chars = path.chars();
    // Drive-qualified only: provider paths (`HKLM:\...`) are not directories.
    if !chars.next()?.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    Some(path)
}

/// Turn an OSC 7 `file://host/path` report into a filesystem path.
fn decode_file_url(pwd: &str) -> String {
    let rest = pwd.strip_prefix("file://").unwrap_or(pwd);
    // Skip the (usually empty) host component; the path starts at the first
    // separator after it.
    let path = match rest.find('/') {
        Some(idx) => &rest[idx..],
        None => rest,
    };
    let decoded = percent_decode(path);
    // Windows reports `/C:/dir`; drop the leading slash and use backslashes.
    if cfg!(target_os = "windows") {
        let bytes = decoded.as_bytes();
        if bytes.len() > 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
            return decoded[1..].replace('/', "\\");
        }
    }
    decoded
}

/// Decode `%XX` escapes in a URL path, keeping multi-byte UTF-8 intact.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::powershell_prompt_path;

    #[test]
    fn powershell_prompt_names_the_directory() {
        assert_eq!(powershell_prompt_path("PS C:\\Windows> "), Some("C:\\Windows"));
        assert_eq!(powershell_prompt_path("PS C:\\a> git status"), Some("C:\\a"));
        assert_eq!(powershell_prompt_path("PS D:\\>"), Some("D:\\"));
    }

    #[test]
    fn other_lines_are_not_powershell_prompts() {
        assert_eq!(powershell_prompt_path("C:\\Windows>"), None);
        assert_eq!(powershell_prompt_path("PS>"), None);
        assert_eq!(powershell_prompt_path("PS HKLM:\\>"), None);
        assert_eq!(powershell_prompt_path(""), None);
    }
}
