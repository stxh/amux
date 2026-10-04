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
/// Handle a keyboard character event with modifier information
/// This implements the comprehensive direct hotkey table (Windows Ctrl maps to Mac ⌘)
pub fn handle_key_input(app: &mut App, key: &str, ctrl: bool, alt: bool, shift: bool) -> bool {
    // ─── Workspaces (Groups) ───
    if ctrl && !shift && !alt && key == "n" {
        app.add_group();
        return true;
    }
    if ctrl && !shift && !alt {
        if let Ok(num) = key.parse::<usize>() {
            if num >= 1 && num <= 8 {
                app.switch_group(num - 1);
                return true;
            }
            if num == 9 {
                if !app.groups.is_empty() {
                    app.switch_group(app.groups.len() - 1);
                }
                return true;
            }
        }
    }
    if ctrl && (alt || shift) && key == "]" {
        app.next_group();
        return true;
    }
    if ctrl && (alt || shift) && key == "[" {
        app.prev_group();
        return true;
    }
    if ctrl && shift && !alt && key == "w" {
        app.remove_group();
        return true;
    }
    if ctrl && shift && !alt && key == "r" {
        app.rename_active_group();
        return true;
    }
    if ctrl && alt && !shift && key == "e" {
        app.edit_active_group_description();
        return true;
    }
    if ctrl && !shift && !alt && key == "b" {
        app.show_sidebar = !app.show_sidebar;
        app.status_message = format!("Sidebar visibility: {}", app.show_sidebar);
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && alt && !shift && key == "b" {
        app.show_right_sidebar = !app.show_right_sidebar;
        app.status_message = format!("Right sidebar visibility: {}", app.show_right_sidebar);
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && shift && !alt && key == "e" {
        app.status_message = "Right sidebar focused".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // ─── Surfaces (Tabs) ───
    if ctrl && !shift && !alt && key == "t" {
        app.add_tab();
        return true;
    }
    if ctrl && shift && !alt && key == "]" {
        app.next_tab();
        return true;
    }
    if ctrl && shift && !alt && key == "[" {
        app.prev_tab();
        return true;
    }
    if ctrl && !shift && !alt && key == "tab" {
        app.next_tab();
        return true;
    }
    if ctrl && shift && !alt && key == "tab" {
        app.prev_tab();
        return true;
    }
    if alt && !ctrl && !shift {
        if let Ok(num) = key.parse::<usize>() {
            if num >= 1 && num <= 8 {
                app.switch_tab(num - 1);
                return true;
            }
            if num == 9 {
                if let Some(group) = app.active_group() {
                    if !group.tabs.is_empty() {
                        app.switch_tab(group.tabs.len() - 1);
                    }
                }
                return true;
            }
        }
    }
    if ctrl && !shift && !alt && key == "w" {
        app.remove_tab();
        return true;
    }

    // ─── Split Panes ───
    if ctrl && !shift && !alt && key == "d" {
        app.split_pane(SplitDirection::Horizontal);
        app.status_message = "Split right".to_string();
        return true;
    }
    if ctrl && shift && !alt && key == "d" {
        app.split_pane(SplitDirection::Vertical);
        app.status_message = "Split down".to_string();
        return true;
    }
    if ctrl && alt && !shift {
        match key {
            "left" => { app.focus_pane_directionally("left"); return true; }
            "right" => { app.focus_pane_directionally("right"); return true; }
            "up" => { app.focus_pane_directionally("up"); return true; }
            "down" => { app.focus_pane_directionally("down"); return true; }
            _ => {}
        }
    }
    if ctrl && shift && !alt && key == "h" {
        app.status_message = "Flash focused pane".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // ─── Browser ───
    if ctrl && shift && !alt && key == "l" {
        app.status_message = "Open browser in split".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "l" {
        app.status_message = "Focus address bar".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "[" {
        app.status_message = "Browser back".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "]" {
        app.status_message = "Browser forward".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "r" {
        app.status_message = "Reload browser page".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && alt && !shift && key == "i" {
        app.status_message = "Toggle Developer Tools".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && alt && !shift && key == "c" {
        app.status_message = "Show JavaScript Console".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // ─── Notifications ───
    if ctrl && !shift && !alt && key == "i" {
        app.status_message = "Show notifications panel".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && shift && !alt && key == "u" {
        app.status_message = "Jump to latest unread notification".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && alt && !shift && key == "u" {
        app.status_message = "Toggle current item unread state".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // ─── Find ───
    if ctrl && !shift && !alt && key == "f" {
        app.status_message = "Find".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && shift && !alt && key == "f" {
        app.status_message = "Find in directory".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "g" {
        app.status_message = "Find next".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && alt && !shift && key == "g" {
        app.status_message = "Find previous".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && alt && shift && key == "f" {
        app.status_message = "Hide find bar".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "e" {
        app.status_message = "Use selection for find".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // ─── Terminal ───
    if ctrl && !shift && !alt && key == "k" {
        if let Some(group) = app.groups.get_mut(app.active_group_index) {
            if let Some(tab) = group.tabs.get_mut(group.active_tab_index) {
                if let Some((pane, _)) = tab.pane_tree.find_mut(tab.active_pane_id) {
                    pane.terminal.scroll_to_bottom();
                    app.redraw_counter += 1;
                    app.status_message = "Cleared scrollback".to_string();
                }
            }
        }
        return true;
    }
    if ctrl && !shift && !alt && key == "v" {
        if let Some(pane) = app.active_pane_mut() {
            let _ = pane.terminal.write_input(b"amux paste buffer");
            app.status_message = "Pasted text".to_string();
            app.redraw_counter += 1;
        }
        return true;
    }
    if ctrl && !shift && !alt && (key == "+" || key == "=") {
        app.font_size = (app.font_size + 1.0).min(72.0);
        app.status_message = format!("Font size: {}", app.font_size);
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "-" {
        app.font_size = (app.font_size - 1.0).max(6.0);
        app.status_message = format!("Font size: {}", app.font_size);
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "0" {
        app.font_size = 18.0;
        app.status_message = "Font size: 18".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // ─── Window ───
    if ctrl && shift && !alt && key == "n" {
        app.status_message = "New window".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && shift && !alt && key == "o" {
        app.status_message = "Reopen previous session".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "," {
        app.status_message = "Settings".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && shift && !alt && key == "," {
        app.status_message = "Reload configuration".to_string();
        app.redraw_counter += 1;
        return true;
    }
    if ctrl && !shift && !alt && key == "q" {
        app.running = false;
        app.status_message = "Quitting...".to_string();
        app.redraw_counter += 1;
        return true;
    }

    // Not a shortcut - forward to active terminal PTY
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
