/// Terminal keyboard input handler - Alacritty-style
///
/// Handles keyboard events similar to how alacritty does:
/// 1. Check for prefix key (Ctrl+B)
/// 2. Handle prefix mode commands (pane, tab, group management)
/// 3. Build ANSI escape sequences for special keys
/// 4. Handle modifier keys (Ctrl, Alt, Shift)
/// 5. Write to terminal PTY

use crate::app::App;
use crate::pane_tree::SplitDirection;
use crate::terminal::Terminal;

/// Handle a keyboard character event with modifier information
/// This follows alacritty's approach to keyboard handling
pub fn handle_key_input(app: &mut App, key: &str, ctrl: bool, alt: bool, shift: bool) -> bool {
    // Check for tmux-like prefix key: Ctrl+B
    if ctrl && !shift && key == "b" {
        app.prefix_mode = !app.prefix_mode;
        app.status_message = if app.prefix_mode {
            "PREFIX - press c/%/\"/n/p/x/t/w/g/1-9".to_string()
        } else {
            "Prefix mode OFF".to_string()
        };
        return true;
    }

    // If in prefix mode, handle prefix commands
    if app.prefix_mode {
        app.prefix_mode = false; // Exit prefix mode after handling

        match key {
            // ── Pane commands ──
            "c" | "%" => {
                app.split_pane(SplitDirection::Vertical);
                app.status_message = "Split vertically".to_string();
                return true;
            }
            "d" | "\"" => {
                app.split_pane(SplitDirection::Horizontal);
                app.status_message = "Split horizontally".to_string();
                return true;
            }
            "n" => {
                app.next_pane();
                app.status_message = "Next pane".to_string();
                return true;
            }
            "p" => {
                app.prev_pane();
                app.status_message = "Previous pane".to_string();
                return true;
            }
            "x" => {
                app.close_pane();
                app.status_message = "Closed pane".to_string();
                return true;
            }
            // ── Tab commands ──
            "t" => {
                app.add_tab();
                return true;
            }
            "w" => {
                app.remove_tab();
                return true;
            }
            // ── Group commands ──
            "g" => {
                app.next_group();
                return true;
            }
            // ── Tab switching with number keys (1-9) ──
            _ => {
                if let Ok(num) = key.parse::<usize>() {
                    if num >= 1 && num <= 9 {
                        app.switch_tab(num - 1);
                        app.status_message = format!("Switched to tab {}", num);
                        return true;
                    }
                }
                app.status_message = format!("Unknown prefix command: {}", key);
                return true;
            }
        }
    }

    // Not in prefix mode - forward to active terminal
    if let Some(pane) = app.active_pane_mut() {
        send_to_terminal(&mut pane.terminal, key, ctrl, alt, shift);
        return true;
    }

    false
}

/// Send key input to terminal PTY - Alacritty-style
/// Converts key events to appropriate escape sequences
fn send_to_terminal(term: &mut Terminal, key: &str, ctrl: bool, alt: bool, shift: bool) {
    // Build modifier flags like alacritty does
    let mods = build_modifiers(ctrl, alt, shift);

    // Check if we should build an escape sequence
    let need_sequence = should_build_sequence(key, ctrl, alt, shift);

    let bytes = if need_sequence {
        build_ansi_sequence(key, mods, ctrl, alt, shift)
    } else {
        // Simple character or text with optional ESC prefix for Alt
        let mut bytes = Vec::with_capacity(key.len() + 1);
        if alt && key.len() == 1 {
            bytes.push(b'\x1b'); // Alt prefix with ESC
        }
        bytes.extend_from_slice(key.as_bytes());
        bytes
    };

    // Write to PTY
    if !bytes.is_empty() {
        let _ = term.write_input(&bytes);
    }
}

/// Build modifier flags bitfield (alacritty-style)
fn build_modifiers(ctrl: bool, alt: bool, shift: bool) -> u8 {
    let mut mods = 0u8;
    if shift {
        mods |= 0b0000_0001; // SHIFT = +1
    }
    if alt {
        mods |= 0b0000_0010; // ALT = +2
    }
    if ctrl {
        mods |= 0b0000_0100; // CONTROL = +4
    }
    mods
}

/// Determine if we should build an escape sequence for this key
fn should_build_sequence(key: &str, ctrl: bool, alt: bool, _shift: bool) -> bool {
    matches!(key,
        "up" | "down" | "left" | "right"
        | "home" | "end" | "pageup" | "pagedown"
        | "insert" | "delete"
        | "f1" | "f2" | "f3" | "f4" | "f5" | "f6"
        | "f7" | "f8" | "f9" | "f10" | "f11" | "f12"
        | "enter" | "backspace" | "escape" | "tab" | "space"
    ) || (ctrl || (alt && key.len() == 1))
}

/// Build ANSI escape sequence for a key with modifiers
/// Following alacritty's approach
fn build_ansi_sequence(key: &str, mods: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    // For simple Ctrl+letter, generate control character
    if ctrl && !alt && key.len() == 1 && key >= "a" && key <= "z" {
        let ctrl_char = (key.as_bytes()[0] - b'a' + 1) as u8;
        return vec![ctrl_char];
    }

    // For special keys, build escape sequences
    let (base, terminator) = match key {
        "up" => ("A", ""),
        "down" => ("B", ""),
        "right" => ("C", ""),
        "left" => ("D", ""),
        "home" => ("H", ""),
        "end" => ("F", ""),
        "pageup" => ("", "5~"),
        "pagedown" => ("", "6~"),
        "insert" => ("", "2~"),
        "delete" => ("", "3~"),
        "f1" => ("P", ""),
        "f2" => ("Q", ""),
        "f3" => ("R", ""),
        "f4" => ("S", ""),
        "f5" => ("", "15~"),
        "f6" => ("", "17~"),
        "f7" => ("", "18~"),
        "f8" => ("", "19~"),
        "f9" => ("", "20~"),
        "f10" => ("", "21~"),
        "f11" => ("", "23~"),
        "f12" => ("", "24~"),
        "enter" => return vec![b'\r'],
        "backspace" => return vec![b'\x7f'],
        "escape" => return vec![b'\x1b'],
        "tab" => {
            if shift {
                return b"\x1b[Z".to_vec(); // Shift+Tab
            }
            return vec![b'\t'];
        }
        "space" => {
            if ctrl {
                return vec![0x00]; // Ctrl+Space = NUL
            }
            return b" ".to_vec();
        }
        _ => return key.as_bytes().to_vec(),
    };

    // Build CSI sequence: \x1b[<base>;<mods><terminator>
    let mut result = Vec::new();
    result.push(b'\x1b'); // ESC
    result.push(b'[');    // CSI

    if !base.is_empty() {
        // Arrow keys: \x1b[<base> or \x1b[1;<mods><base>
        if mods != 0 {
            result.extend_from_slice(b"1;");
            // Encode modifiers: 1 + mod_bits
            let mod_num = mods + 1;
            result.extend_from_slice(format!("{}", mod_num).as_bytes());
        }
        result.extend_from_slice(base.as_bytes());
    } else {
        // Function keys with ~: \x1b[<number>;<mods>~
        result.extend_from_slice(terminator[..terminator.len()-1].as_bytes());
        if mods != 0 {
            let mod_num = mods + 1;
            result.push(b';');
            result.extend_from_slice(format!("{}", mod_num).as_bytes());
        }
        result.push(terminator.as_bytes()[terminator.len()-1]);
    }

    result
}

/// Handle text input from IME or direct character input
pub fn handle_text_input(app: &mut App, text: &str) {
    if text.is_empty() {
        return;
    }

    // In prefix mode, first character is the command
    if app.prefix_mode {
        app.prefix_mode = false;
        if let Some(first_char) = text.chars().next() {
            let key = first_char.to_string();
            handle_key_input(app, &key, false, false, false);
        }
        return;
    }

    // Forward to active terminal
    if let Some(pane) = app.active_pane_mut() {
        let _ = pane.terminal.write_input(text.as_bytes());
    }
}
