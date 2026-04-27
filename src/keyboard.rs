/// Keyboard event handling for terminal multiplexer
/// Routes keyboard input to the active terminal's PTY

use crate::app::App;
use crate::pane::LayoutMode;
use crate::terminal::Terminal;

/// Handle a keyboard character event
/// Returns true if the event was handled (consumed)
pub fn handle_key_input(app: &mut App, key: &str, ctrl: bool, alt: bool) -> bool {
    // Check for tmux-like prefix key: Ctrl+B
    if ctrl && key == "b" {
        app.prefix_mode = !app.prefix_mode;
        app.status_message = if app.prefix_mode {
            "Prefix mode ON - press c/d/n/p/x".to_string()
        } else {
            "Prefix mode OFF".to_string()
        };
        return true;
    }

    // If in prefix mode, handle prefix commands
    if app.prefix_mode {
        app.prefix_mode = false; // Exit prefix mode after handling
        
        match key {
            "c" => {
                app.split_pane(LayoutMode::Vertical);
                return true;
            }
            "%" => {
                app.split_pane(LayoutMode::Horizontal);
                return true;
            }
            "\"" => {
                app.split_pane(LayoutMode::Vertical);
                return true;
            }
            "d" => {
                // Detach (not implemented yet, just show message)
                app.status_message = "Detach not implemented".to_string();
                return true;
            }
            "n" => {
                app.next_pane();
                return true;
            }
            "p" => {
                app.prev_pane();
                return true;
            }
            "x" => {
                app.close_pane();
                return true;
            }
            "z" => {
                // Suspend (not implemented)
                app.status_message = "Suspend not implemented".to_string();
                return true;
            }
            _ => {
                app.status_message = format!("Unknown prefix command: {}", key);
                return true;
            }
        }
    }

    // Not in prefix mode - forward to active terminal
    if let Some(pane) = app.active_pane_mut() {
        send_to_terminal(&mut pane.terminal, key, ctrl, alt);
        return true;
    }

    false
}

/// Send key input to terminal PTY
fn send_to_terminal(term: &mut Terminal, key: &str, ctrl: bool, alt: bool) {
    if ctrl {
        // Ctrl+letter -> control character
        if key.len() == 1 && key >= "a" && key <= "z" {
            let ctrl_char = (key.as_bytes()[0] - b'a' + 1) as char;
            let _ = term.write_input(&[ctrl_char as u8]);
            return;
        }
        
        match key {
            "space" => { let _ = term.write_input(&[0x00]); },
            "2" => { let _ = term.write_input(&[0x00]); }, // Ctrl+Space = Ctrl+@
            "@" => { let _ = term.write_input(&[0x00]); }, // Ctrl+Space
            "[" => { let _ = term.write_input(&[0x1b]); }, // Ctrl+[ = Escape
            "3" => { let _ = term.write_input(&[0x1b]); },
            "\\" => { let _ = term.write_input(&[0x1c]); }, // Ctrl+\
            "4" => { let _ = term.write_input(&[0x1c]); },
            "]" => { let _ = term.write_input(&[0x1d]); }, // Ctrl+]
            "5" => { let _ = term.write_input(&[0x1d]); },
            "6" => { let _ = term.write_input(&[0x1e]); }, // Ctrl+^
            "^" => { let _ = term.write_input(&[0x1e]); },
            "-" => { let _ = term.write_input(&[0x1f]); }, // Ctrl+_
            "7" => { let _ = term.write_input(&[0x1f]); },
            "8" => { let _ = term.write_input(&[0x7f]); }, // Ctrl+Backspace = DEL
            "?" => { let _ = term.write_input(&[0x7f]); }, // Ctrl+? = DEL
            "c" => { let _ = term.write_input(&[0x03]); }, // Ctrl+C
            "d" => { let _ = term.write_input(&[0x04]); }, // Ctrl+D
            "z" => { let _ = term.write_input(&[0x1a]); }, // Ctrl+Z
            _ => {}
        }
    } else if alt {
        // Alt+letter -> Escape sequence
        if key.len() == 1 {
            let _ = term.write_input(&[0x1b]); // Escape
            let _ = term.write_input(key.as_bytes());
            return;
        }
    } else {
        // Regular key
        match key {
            "enter" | "return" => {
                let _ = term.write_input(b"\r");
            }
            "backspace" | "delete" => {
                let _ = term.write_input(b"\x7f");
            }
            "escape" => {
                let _ = term.write_input(b"\x1b");
            }
            "tab" => {
                let _ = term.write_input(b"\t");
            }
            "space" => {
                let _ = term.write_input(b" ");
            }
            "up" => {
                let _ = term.write_input(b"\x1b[A");
            }
            "down" => {
                let _ = term.write_input(b"\x1b[B");
            }
            "right" => {
                let _ = term.write_input(b"\x1b[C");
            }
            "left" => {
                let _ = term.write_input(b"\x1b[D");
            }
            "home" => {
                let _ = term.write_input(b"\x1b[H");
            }
            "end" => {
                let _ = term.write_input(b"\x1b[F");
            }
            "pageup" => {
                let _ = term.write_input(b"\x1b[5~");
            }
            "pagedown" => {
                let _ = term.write_input(b"\x1b[6~");
            }
            "insert" => {
                let _ = term.write_input(b"\x1b[2~");
            }
            "delete" => {
                let _ = term.write_input(b"\x1b[3~");
            }
            "f1" => {
                let _ = term.write_input(b"\x1bOP");
            }
            "f2" => {
                let _ = term.write_input(b"\x1bOQ");
            }
            "f3" => {
                let _ = term.write_input(b"\x1bOR");
            }
            "f4" => {
                let _ = term.write_input(b"\x1bOS");
            }
            "f5" => {
                let _ = term.write_input(b"\x1b[15~");
            }
            "f6" => {
                let _ = term.write_input(b"\x1b[17~");
            }
            "f7" => {
                let _ = term.write_input(b"\x1b[18~");
            }
            "f8" => {
                let _ = term.write_input(b"\x1b[19~");
            }
            "f9" => {
                let _ = term.write_input(b"\x1b[20~");
            }
            "f10" => {
                let _ = term.write_input(b"\x1b[21~");
            }
            "f11" => {
                let _ = term.write_input(b"\x1b[23~");
            }
            "f12" => {
                let _ = term.write_input(b"\x1b[24~");
            }
            _ => {
                // Regular character
                if key.len() == 1 {
                    let _ = term.write_input(key.as_bytes());
                }
            }
        }
    }
}
