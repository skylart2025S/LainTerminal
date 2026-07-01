//! Translation of egui keyboard events into the byte sequences a terminal
//! expects, so keystrokes can be forwarded straight to the PTY.

use eframe::egui::{Key, Modifiers};

/// Encode a pressed key into terminal bytes. Returns `None` for keys that are
/// already delivered as `Event::Text` (ordinary character typing).
pub fn encode_key(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // Control combinations: Ctrl+A..Z -> 0x01..0x1a, plus a few symbols.
    if let Some(b) = ctrl_byte(key).filter(|_| mods.ctrl) {
        return Some(vec![b]);
    }

    let bytes: &[u8] = match key {
        Key::Enter => b"\r",
        Key::Backspace => b"\x7f",
        Key::Tab if mods.shift => b"\x1b[Z",
        Key::Tab => b"\t",
        Key::Escape => b"\x1b",
        Key::Delete => b"\x1b[3~",
        Key::Insert => b"\x1b[2~",
        Key::PageUp => b"\x1b[5~",
        Key::PageDown => b"\x1b[6~",
        Key::ArrowUp => cursor_seq(b'A', app_cursor),
        Key::ArrowDown => cursor_seq(b'B', app_cursor),
        Key::ArrowRight => cursor_seq(b'C', app_cursor),
        Key::ArrowLeft => cursor_seq(b'D', app_cursor),
        Key::Home => cursor_seq(b'H', app_cursor),
        Key::End => cursor_seq(b'F', app_cursor),
        _ => return None,
    };
    Some(bytes.to_vec())
}

/// Arrow/Home/End: `ESC [ X` normally, `ESC O X` in application-cursor mode.
fn cursor_seq(final_byte: u8, app_cursor: bool) -> &'static [u8] {
    match (final_byte, app_cursor) {
        (b'A', false) => b"\x1b[A",
        (b'A', true) => b"\x1bOA",
        (b'B', false) => b"\x1b[B",
        (b'B', true) => b"\x1bOB",
        (b'C', false) => b"\x1b[C",
        (b'C', true) => b"\x1bOC",
        (b'D', false) => b"\x1b[D",
        (b'D', true) => b"\x1bOD",
        (b'H', false) => b"\x1b[H",
        (b'H', true) => b"\x1bOH",
        (b'F', false) => b"\x1b[F",
        (b'F', true) => b"\x1bOF",
        _ => b"",
    }
}

fn ctrl_byte(key: Key) -> Option<u8> {
    // Note: Ctrl+C, Ctrl+X and Ctrl+V are intentionally omitted here — egui
    // delivers them as Copy/Cut/Paste events instead of key presses, so they
    // are handled there to avoid missing or double-sending them.
    let b = match key {
        Key::A => 1,
        Key::B => 2,
        Key::D => 4,
        Key::E => 5,
        Key::F => 6,
        Key::G => 7,
        Key::H => 8,
        Key::I => 9,
        Key::J => 10,
        Key::K => 11,
        Key::L => 12,
        Key::M => 13,
        Key::N => 14,
        Key::O => 15,
        Key::P => 16,
        Key::Q => 17,
        Key::R => 18,
        Key::S => 19,
        Key::T => 20,
        Key::U => 21,
        Key::W => 23,
        Key::Y => 25,
        Key::Z => 26,
        Key::OpenBracket => 0x1b,
        Key::Backslash => 0x1c,
        Key::CloseBracket => 0x1d,
        Key::Space => 0,
        _ => return None,
    };
    Some(b)
}
