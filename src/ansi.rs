//! Terminal emulation backed by the `vt100` crate.
//!
//! `vt100` maintains a full grid (cursor, colors, alternate screen, scroll
//! regions, etc.), which is what lets real TUI programs like vim and nano work.
//! We render its screen directly with the egui painter.
//!
//! Alongside it we run a tiny `vte` parser that only sniffs OSC 133 shell
//! integration markers, so Lain's mood can still react to command execution.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Stroke, Vec2};
use vte::{Params, Perform};

/// Lain's current emotional state, derived from command execution.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mood {
    Neutral,
    Thinking,
    Happy,
    Sad,
    Upset,
    /// Shown while the user is typing (Lain is watching).
    Watching,
}

impl Mood {
    pub fn label(self) -> &'static str {
        match self {
            Mood::Neutral => "present day",
            Mood::Thinking => "connecting...",
            Mood::Happy => "let's all love lain",
            Mood::Sad => "close the world",
            Mood::Upset => "signal lost",
            Mood::Watching => "watching...",
        }
    }
}

// --- Theme -----------------------------------------------------------------

pub const BG: Color32 = Color32::from_rgb(12, 12, 18);
pub const FG: Color32 = Color32::from_rgb(198, 200, 214);

const PALETTE: [Color32; 16] = [
    Color32::from_rgb(28, 28, 36),
    Color32::from_rgb(224, 90, 90),
    Color32::from_rgb(140, 200, 130),
    Color32::from_rgb(214, 190, 120),
    Color32::from_rgb(120, 160, 220),
    Color32::from_rgb(190, 140, 210),
    Color32::from_rgb(120, 200, 210),
    Color32::from_rgb(198, 200, 214),
    Color32::from_rgb(90, 92, 104),
    Color32::from_rgb(240, 120, 120),
    Color32::from_rgb(160, 220, 150),
    Color32::from_rgb(232, 210, 140),
    Color32::from_rgb(150, 185, 240),
    Color32::from_rgb(210, 165, 230),
    Color32::from_rgb(150, 220, 230),
    Color32::from_rgb(235, 237, 245),
];

const SCROLLBACK: usize = 2000;

/// A terminal: a `vt100` grid plus an OSC 133 mood sniffer.
pub struct Terminal {
    parser: vt100::Parser,
    sniff: vte::Parser,
    sniffer: MoodSniffer,
}

impl Terminal {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK),
            sniff: vte::Parser::new(),
            sniffer: MoodSniffer::new(),
        }
    }

    /// Feed raw PTY output into the emulator (and the mood sniffer).
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        for &b in bytes {
            self.sniff.advance(&mut self.sniffer, b);
        }
    }

    pub fn mood(&self) -> Mood {
        self.sniffer.mood
    }

    /// Build replies to any terminal queries seen since the last call (cursor
    /// position report, device status, device attributes). These must be
    /// written back to the PTY so programs like vim don't stall waiting.
    pub fn take_responses(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if std::mem::take(&mut self.sniffer.report_status) {
            out.extend_from_slice(b"\x1b[0n");
        }
        if std::mem::take(&mut self.sniffer.report_cursor) {
            let (row, col) = self.parser.screen().cursor_position();
            out.extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
        }
        if std::mem::take(&mut self.sniffer.report_da) {
            // Primary device attributes: "VT100 with advanced video".
            out.extend_from_slice(b"\x1b[?1;2c");
        }
        out
    }

    pub fn set_mood(&mut self, mood: Mood) {
        self.sniffer.mood = mood;
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Whether the app is in "application cursor keys" mode (affects how arrow
    /// keys should be encoded).
    pub fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    /// True when a full-screen app (vim, nano, ...) is using the alternate
    /// screen buffer. We give those a solid backdrop for readability.
    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Paint the grid (and cursor) into `rect`. Cells are laid out on a fixed
    /// monospace grid of `char_w` x `line_h` pixels.
    pub fn render(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        char_w: f32,
        line_h: f32,
        font_size: f32,
        time: f32,
    ) {
        let font = egui::FontId::monospace(font_size);
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();

        for row in 0..rows {
            let y = rect.min.y + row as f32 * line_h;
            let mut col = 0u16;
            while col < cols {
                // Skip the trailing half of wide (CJK) glyphs.
                if screen
                    .cell(row, col)
                    .map(|c| c.is_wide_continuation())
                    .unwrap_or(false)
                {
                    col += 1;
                    continue;
                }

                let style = screen.cell(row, col).map(run_style).unwrap_or_default();
                let start = col;
                let mut text = String::new();

                // Coalesce a run of adjacent cells sharing the same style.
                while col < cols {
                    let Some(cell) = screen.cell(row, col) else {
                        break;
                    };
                    if cell.is_wide_continuation() {
                        col += 1;
                        continue;
                    }
                    if run_style(cell) != style {
                        break;
                    }
                    let c = cell.contents();
                    text.push_str(if c.is_empty() { " " } else { c });
                    col += 1;
                }

                let x = rect.min.x + start as f32 * char_w;
                let run_w = (col - start) as f32 * char_w;

                if let Some(bg) = style.bg {
                    painter.rect_filled(
                        Rect::from_min_size(Pos2::new(x, y), Vec2::new(run_w, line_h)),
                        0.0,
                        bg,
                    );
                }
                painter.text(Pos2::new(x, y), Align2::LEFT_TOP, &text, font.clone(), style.fg);
                if style.underline {
                    painter.hline(x..=x + run_w, y + line_h - 1.0, Stroke::new(1.0, style.fg));
                }
            }
        }

        self.draw_cursor(painter, rect, char_w, line_h, &font, time);
    }

    fn draw_cursor(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        char_w: f32,
        line_h: f32,
        font: &egui::FontId,
        time: f32,
    ) {
        let screen = self.parser.screen();
        if screen.hide_cursor() {
            return;
        }
        // Blink: on ~65% of a ~1s cycle.
        if (time * 1.4).rem_euclid(1.0) > 0.65 {
            return;
        }

        let (crow, ccol) = screen.cursor_position();
        let x = rect.min.x + ccol as f32 * char_w;
        let y = rect.min.y + crow as f32 * line_h;
        let cur_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(char_w, line_h));
        painter.rect_filled(cur_rect, 0.0, FG);

        // Redraw the glyph under the cursor in the background color.
        if let Some(cell) = screen.cell(crow, ccol) {
            let c = cell.contents();
            if !c.is_empty() {
                painter.text(Pos2::new(x, y), Align2::LEFT_TOP, c, font.clone(), BG);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct RunStyle {
    fg: Color32,
    bg: Option<Color32>,
    underline: bool,
}

impl Default for RunStyle {
    fn default() -> Self {
        Self {
            fg: FG,
            bg: None,
            underline: false,
        }
    }
}

fn run_style(cell: &vt100::Cell) -> RunStyle {
    let mut fg = convert_color(cell.fgcolor(), FG);
    let mut bg = match cell.bgcolor() {
        vt100::Color::Default => None,
        other => Some(convert_color(other, BG)),
    };

    if cell.bold() {
        fg = brighten(fg);
    }
    if cell.inverse() {
        let real_bg = bg.unwrap_or(BG);
        bg = Some(fg);
        fg = real_bg;
    }

    RunStyle {
        fg,
        bg,
        underline: cell.underline(),
    }
}

fn convert_color(color: vt100::Color, default: Color32) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => color_256(i),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Nudge one of the base 8 colors toward its bright variant (for bold text).
fn brighten(c: Color32) -> Color32 {
    for i in 0..8 {
        if PALETTE[i] == c {
            return PALETTE[i + 8];
        }
    }
    c
}

fn color_256(idx: u8) -> Color32 {
    match idx {
        0..=15 => PALETTE[idx as usize],
        16..=231 => {
            let i = idx - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let scale = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(scale(r), scale(g), scale(b))
        }
        _ => {
            let v = 8 + (idx - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

// --- OSC 133 mood sniffer --------------------------------------------------

struct MoodSniffer {
    mood: Mood,
    rng: u64,
    report_cursor: bool,
    report_status: bool,
    report_da: bool,
}

impl MoodSniffer {
    fn new() -> Self {
        Self {
            mood: Mood::Neutral,
            rng: seed_rng(),
            report_cursor: false,
            report_status: false,
            report_da: false,
        }
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

impl Perform for MoodSniffer {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.first().map(|p| *p == b"133").unwrap_or(false) {
            match params.get(1) {
                Some(&b"C") => self.mood = Mood::Thinking,
                Some(&b"D") => {
                    let code = params
                        .get(2)
                        .and_then(|c| std::str::from_utf8(c).ok())
                        .and_then(|c| c.trim().parse::<i32>().ok())
                        .unwrap_or(0);
                    self.mood = if code == 0 {
                        Mood::Happy
                    } else if self.next_rand() & 1 == 0 {
                        Mood::Sad
                    } else {
                        Mood::Upset
                    };
                }
                _ => {}
            }
        }
    }

    // We only care about OSC;
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let first = params.iter().next().map(|p| *p.first().unwrap_or(&0));
        match action {
            // Device status report: 5 = status, 6 = cursor position.
            'n' => match first {
                Some(5) => self.report_status = true,
                Some(6) => self.report_cursor = true,
                _ => {}
            },
            // Primary device attributes (only the plain `ESC [ c` form).
            'c' if intermediates.is_empty() => self.report_da = true,
            _ => {}
        }
    }

    fn print(&mut self, _c: char) {}
    fn execute(&mut self, _byte: u8) {}
    fn esc_dispatch(&mut self, _i: &[u8], _ig: bool, _b: u8) {}
}

fn seed_rng() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15);
    nanos | 1
}
