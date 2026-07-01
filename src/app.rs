//! The egui application: theming, the boot splash, and the terminal view.

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui::{self, Color32};

use crate::ansi::{Mood, Terminal, BG, FG};
use crate::input::encode_key;
use crate::lain;
use crate::stats::{self, SysStats};
use crate::video::VideoPlayer;

/// Path to the optional startup splash video.
const BOOT_VIDEO: &str = "sprites-backgrounds/copeland_os.mp4";

/// Path to the optional terminal background image.
const BACKGROUND: &str = "sprites-backgrounds/copeland_background.png";

/// How long a command's result expression (happy/sad/upset) lingers before
/// Lain returns to her neutral/idle state (seconds).
const RESULT_HOLD: f32 = 3.0;

/// How long the startup splash animation plays before the terminal appears.
const BOOT_DURATION: f32 = 10.0;

/// How long after the last keystroke Lain keeps "listening" before idling.
const TYPING_WINDOW: f32 = 1.2;

/// Padding between the window edge and the terminal grid.
const PAD: egui::Vec2 = egui::vec2(10.0, 8.0);

/// Shared "Navi" UI palette.
const ACCENT: Color32 = Color32::from_rgb(150, 220, 230);
const DIM: Color32 = Color32::from_rgb(110, 112, 130);
const HOT: Color32 = Color32::from_rgb(230, 120, 150);
const RED: Color32 = Color32::from_rgb(240, 110, 110);

pub struct LainTerminal {
    terminal: Terminal,
    out_rx: Receiver<Vec<u8>>,
    in_tx: Sender<Vec<u8>>,
    resize_tx: Sender<(u16, u16)>,
    font_size: f32,
    theme_installed: bool,
    /// Current grid size (rows, cols); resends a resize when it changes.
    grid_size: (u16, u16),
    /// Drives the idle "breathing"/blink animation for Lain and the cursor.
    anim_time: f32,
    avatar: lain::Avatar,
    /// Used to detect when a new result mood appears, so we can time its hold.
    last_mood: Mood,
    /// `anim_time` after which a result expression reverts to Neutral.
    result_deadline: Option<f32>,
    /// True while the startup splash animation is playing.
    booting: bool,
    /// Optional video splash; takes precedence over the GIF splash.
    boot_video: Option<VideoPlayer>,
    /// Optional terminal background image (loaded lazily on first frame).
    background: Option<egui::TextureHandle>,
    background_loaded: bool,
    /// Real system stats sampled from /proc.
    stats: SysStats,
    /// `anim_time` of the last keystroke, for the typing reaction.
    last_typed: f32,
}

impl LainTerminal {
    pub fn new(
        out_rx: Receiver<Vec<u8>>,
        in_tx: Sender<Vec<u8>>,
        resize_tx: Sender<(u16, u16)>,
    ) -> Self {
        Self {
            terminal: Terminal::new(40, 120),
            out_rx,
            in_tx,
            resize_tx,
            font_size: 14.0,
            theme_installed: false,
            grid_size: (40, 120),
            anim_time: 0.0,
            avatar: lain::Avatar::default(),
            last_mood: Mood::Neutral,
            result_deadline: None,
            booting: true,
            boot_video: VideoPlayer::start(BOOT_VIDEO),
            background: None,
            background_loaded: false,
            stats: SysStats::new(),
            last_typed: -10.0,
        }
    }

    /// Pull any pending PTY output and feed it through the emulator, then reply
    /// to any terminal queries (cursor position, device attributes, ...).
    fn pump_output(&mut self) {
        while let Ok(chunk) = self.out_rx.try_recv() {
            self.terminal.process(&chunk);
        }
        let responses = self.terminal.take_responses();
        if !responses.is_empty() {
            let _ = self.in_tx.send(responses);
        }
    }

    /// Collect keyboard/paste input and forward it to the PTY as raw bytes.
    fn forward_input(&mut self, ctx: &egui::Context) {
        let app_cursor = self.terminal.application_cursor();
        let mut bytes: Vec<u8> = Vec::new();

        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Text(text) => bytes.extend_from_slice(text.as_bytes()),
                    egui::Event::Paste(text) => bytes.extend_from_slice(text.as_bytes()),
                    // egui turns Ctrl+C/X into Copy/Cut events rather than key
                    // presses; forward them as the raw control bytes a terminal
                    // expects (Ctrl+C = SIGINT, Ctrl+X = 0x18).
                    egui::Event::Copy => bytes.push(0x03),
                    egui::Event::Cut => bytes.push(0x18),
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if let Some(seq) = encode_key(*key, *modifiers, app_cursor) {
                            bytes.extend_from_slice(&seq);
                        }
                    }
                    _ => {}
                }
            }
        });

        if !bytes.is_empty() {
            self.last_typed = self.anim_time;
            let _ = self.in_tx.send(bytes);
        }
    }

    /// Render the full-screen startup splash animation (video if available,
    /// otherwise the GIF).
    fn show_boot(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter().clone();
                match &mut self.boot_video {
                    Some(video) => video.draw(ctx, &painter, rect),
                    None => self.avatar.draw_boot(&painter, rect, self.anim_time),
                }
                painter.text(
                    rect.center_bottom() + egui::vec2(0.0, -24.0),
                    egui::Align2::CENTER_BOTTOM,
                    "press any key",
                    egui::FontId::monospace(12.0),
                    Color32::from_rgba_unmultiplied(200, 200, 214, 160),
                );
            });
    }

    /// Real system stats (CPU / memory / network / load) from `/proc`.
    fn draw_meters(&self, ui: &mut egui::Ui) {
        let s = &self.stats;

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("// navi status")
                .monospace()
                .size(11.0)
                .color(ACCENT),
        );
        ui.add_space(8.0);

        stat_bar(ui, "cpu", s.cpu_pct, format!("{:.0}%", s.cpu_pct));
        stat_bar(
            ui,
            "mem",
            s.mem_pct,
            format!("{}/{}", stats::fmt_size(s.mem_used), stats::fmt_size(s.mem_total)),
        );

        ui.add_space(6.0);
        let line = |ui: &mut egui::Ui, s: String| {
            ui.label(egui::RichText::new(s).monospace().size(10.0).color(DIM));
        };
        line(ui, format!("net dn  {}", stats::fmt_rate(s.rx_rate)));
        line(ui, format!("net up  {}", stats::fmt_rate(s.tx_rate)));
        line(ui, format!("load    {:.2}", s.load1));
    }

    /// A small "Navi"-style status block pinned to the bottom of the panel.
    fn draw_status(&self, ui: &mut egui::Ui) {
        let line = |ui: &mut egui::Ui, s: String, c: Color32| {
            ui.label(egui::RichText::new(s).monospace().size(11.0).color(c));
        };

        let (rows, cols) = self.grid_size;
        let secs = self.anim_time as u64;

        // bottom_up lays items out from the bottom edge upward.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(2.0);
            line(ui, "protocol 7 // wired".into(), DIM);
            line(ui, format!("grid    {cols}x{rows}"), DIM);
            line(ui, format!("uptime  {:02}:{:02}", secs / 60, secs % 60), DIM);
            line(ui, "NAVI".into(), ACCENT);
            ui.add_space(6.0);
            ui.separator();
        });
    }

    /// Hold a command's result expression for a beat, then relax to Neutral.
    fn update_mood_timer(&mut self) {
        let mood = self.terminal.mood();
        if mood != self.last_mood {
            self.last_mood = mood;
            self.result_deadline = match mood {
                Mood::Happy | Mood::Sad | Mood::Upset => Some(self.anim_time + RESULT_HOLD),
                _ => None,
            };
        }
        if self.result_deadline.is_some_and(|deadline| self.anim_time >= deadline) {
            self.terminal.set_mood(Mood::Neutral);
            self.last_mood = Mood::Neutral;
            self.result_deadline = None;
        }
    }

    /// Recompute the grid from the available area and resize if it changed.
    fn sync_size(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let font_id = egui::FontId::monospace(self.font_size);
        let char_w = ctx.fonts(|f| f.glyph_width(&font_id, 'M')).max(1.0);
        let line_h = ctx.fonts(|f| f.row_height(&font_id)).max(1.0);

        let cols = ((rect.width() / char_w).floor() as u16).max(1);
        let rows = ((rect.height() / line_h).floor() as u16).max(1);

        if (rows, cols) != self.grid_size {
            self.grid_size = (rows, cols);
            self.terminal.set_size(rows, cols);
            let _ = self.resize_tx.send((rows, cols));
        }
    }

    /// A stylized "Navi" window frame: a mood-tinted pulsing glow border,
    /// bright corner brackets, a `COPLAND OS` header tag, a divider before the
    /// Lain column, and faint CRT scanlines over that column. Drawn on top of
    /// all panels since the window itself is undecorated.
    fn draw_frame(&self, ctx: &egui::Context, mood: Mood) {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("navi_frame"),
        ));
        let screen = ctx.screen_rect();
        let frame = screen.shrink(5.0);
        let rounding = egui::Rounding::same(6.0);

        let accent = Color32::from_rgb(120, 200, 210);
        // Border tint drifts toward the current mood color and gently pulses.
        let tint = blend(accent, mood_color(mood), 0.5);
        let pulse = 0.5 + 0.5 * (self.anim_time * 2.2).sin();
        let glow = tint.gamma_multiply(0.15 + 0.35 * pulse);
        let bright = blend(tint, Color32::WHITE, 0.25);

        // Outer glow + crisp border.
        painter.rect_stroke(frame.expand(2.0), rounding, egui::Stroke::new(4.0, glow));
        painter.rect_stroke(frame, rounding, egui::Stroke::new(1.2, tint));

        // Corner brackets.
        let len = 20.0;
        let stroke = egui::Stroke::new(2.5, bright);
        let corner = |p: egui::Pos2, dx: f32, dy: f32| {
            painter.line_segment([p, p + egui::vec2(dx * len, 0.0)], stroke);
            painter.line_segment([p, p + egui::vec2(0.0, dy * len)], stroke);
        };
        corner(frame.left_top(), 1.0, 1.0);
        corner(frame.right_top(), -1.0, 1.0);
        corner(frame.left_bottom(), 1.0, -1.0);
        corner(frame.right_bottom(), -1.0, -1.0);

        // Header tag inset into the top border.
        let font = egui::FontId::monospace(11.0);
        let galley = painter.layout_no_wrap("◈ COPLAND OS ◈".to_owned(), font, bright);
        let size = galley.size();
        let center = egui::pos2(screen.center().x, frame.top());
        let chip = egui::Rect::from_center_size(center, egui::vec2(size.x + 16.0, size.y + 5.0));
        painter.rect_filled(chip, egui::Rounding::same(3.0), Color32::from_rgb(10, 10, 16));
        painter.rect_stroke(chip, egui::Rounding::same(3.0), egui::Stroke::new(1.0, tint));
        painter.galley(center - size / 2.0, galley, bright);

        // Divider before the Lain column (panel is 240px on the right).
        let div_x = screen.right() - 240.0;
        if div_x > frame.left() + 40.0 {
            painter.line_segment(
                [
                    egui::pos2(div_x, frame.top() + 8.0),
                    egui::pos2(div_x, frame.bottom() - 8.0),
                ],
                egui::Stroke::new(1.0, tint.gamma_multiply(0.6)),
            );
            painter.circle_filled(egui::pos2(div_x, frame.top()), 2.5, bright);

            // Faint CRT scanlines over the Lain column.
            let scan = Color32::from_rgba_unmultiplied(0, 0, 0, 55);
            let mut y = frame.top() + 4.0;
            while y < frame.bottom() - 4.0 {
                painter.hline(div_x..=frame.right(), y, egui::Stroke::new(1.0, scan));
                y += 3.0;
            }
        }
    }

    /// Custom window controls (the window is undecorated): a draggable top
    /// strip plus minimize / maximize / close buttons.
    fn draw_window_controls(&self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let screen = ctx.screen_rect();

        // Draggable title strip across the top (double-click to maximize).
        egui::Area::new(egui::Id::new("titlebar_drag"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.left_top())
            .show(ctx, |ui| {
                let (_rect, resp) = ui.allocate_exact_size(
                    egui::vec2(screen.width() - 110.0, 18.0),
                    egui::Sense::click_and_drag(),
                );
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
            });

        // Buttons in the top-right, inside the border.
        egui::Area::new(egui::Id::new("win_buttons"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 5.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if win_button(ui, "\u{2013}", ACCENT) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    if win_button(ui, "\u{25a1}", ACCENT) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if win_button(ui, "\u{00d7}", RED) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }

    fn install_theme(&mut self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = BG;
        style.visuals.override_text_color = Some(FG);
        ctx.set_style(style);
    }
}

impl eframe::App for LainTerminal {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_installed {
            self.install_theme(ctx);
            self.theme_installed = true;
        }

        // Skip the (large) GIF splash when a video splash is present.
        self.avatar
            .load(ctx, "sprites-backgrounds", self.boot_video.is_none());

        if !self.background_loaded {
            self.background = lain::load_texture(ctx, BACKGROUND);
            self.background_loaded = true;
        }

        self.anim_time += ctx.input(|i| i.stable_dt).min(0.1);
        self.pump_output();

        // Quit on Ctrl+Shift+Q. (Esc is left for the terminal, so vim works.)
        let quit = ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Q));
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // --- Startup splash ---------------------------------------------
        if self.booting {
            let have_splash = self.boot_video.is_some() || self.avatar.has_boot();
            let skip = ctx.input(|i| {
                i.pointer.any_pressed()
                    || i.events
                        .iter()
                        .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
            });
            if !have_splash || self.anim_time >= BOOT_DURATION || skip {
                self.booting = false;
                self.boot_video = None; // stops ffmpeg (see Drop impl)
            } else {
                self.show_boot(ctx);
                ctx.request_repaint();
                return;
            }
        }

        self.forward_input(ctx);
        self.update_mood_timer();
        self.stats.update();

        // Paint one continuous background across the whole window, behind both
        // panels (which are transparent), so they share the same backdrop.
        {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let screen = ctx.screen_rect();
            match &self.background {
                Some(bg) => {
                    lain::draw_cover(&painter, screen, bg);
                    painter.rect_filled(
                        screen,
                        0.0,
                        Color32::from_rgba_unmultiplied(6, 6, 10, 175),
                    );
                }
                None => {
                    painter.rect_filled(screen, 0.0, BG);
                }
            }
        }

        // Display mood. Lain "watches" while you type. Inside a full-screen app
        // (vim/nano) she watches on keystrokes and idles when you pause;
        // otherwise command-execution moods take priority over the typing hint.
        let base = self.terminal.mood();
        let typing = self.anim_time - self.last_typed < TYPING_WINDOW;
        let (mood, mood_label): (Mood, &str) = if self.terminal.alternate_screen() {
            if typing {
                (Mood::Watching, Mood::Watching.label())
            } else {
                (Mood::Neutral, Mood::Neutral.label())
            }
        } else if base == Mood::Neutral && typing {
            (Mood::Watching, Mood::Watching.label())
        } else {
            (base, base.label())
        };

        // --- Lain side panel --------------------------------------------
        egui::SidePanel::right("lain_panel")
            .exact_width(240.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("L A I N")
                            .monospace()
                            .size(18.0)
                            .color(Color32::from_rgb(150, 220, 230)),
                    );
                    ui.add_space(12.0);

                    let side = ui.available_width();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                    self.avatar
                        .draw_background(ui.painter(), rect, mood, self.anim_time);

                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new(mood_label)
                            .monospace()
                            .size(13.0)
                            .color(mood_color(mood)),
                    );
                });

                // Fill the middle with animated status meters, and pin a small
                // readout to the very bottom.
                self.draw_meters(ui);
                self.draw_status(ui);
            });

        // --- Terminal ---------------------------------------------------
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let full = ui.max_rect();

                // Full-screen apps (vim/nano) get a solid backdrop so the
                // background image/scanlines don't bleed through their UI.
                if self.terminal.alternate_screen() {
                    ui.painter().rect_filled(full, 0.0, BG);
                }

                // Leave a little extra headroom at the top for the header tag.
                let term_rect = egui::Rect::from_min_max(
                    full.min + egui::vec2(PAD.x, 20.0),
                    full.max - PAD,
                );
                self.sync_size(ctx, term_rect);

                let font_id = egui::FontId::monospace(self.font_size);
                let char_w = ctx.fonts(|f| f.glyph_width(&font_id, 'M')).max(1.0);
                let line_h = ctx.fonts(|f| f.row_height(&font_id)).max(1.0);
                self.terminal
                    .render(ui.painter(), term_rect, char_w, line_h, self.font_size, self.anim_time);

                // Keep keyboard focus on the terminal so keys are captured.
                let resp = ui.interact(
                    full,
                    ui.id().with("terminal"),
                    egui::Sense::click_and_drag(),
                );
                if !resp.has_focus() {
                    resp.request_focus();
                }
            });

        self.draw_frame(ctx, mood);
        self.draw_window_controls(ctx);
        ctx.request_repaint();
    }
}

/// One labelled percentage bar with a value string on the right.
fn stat_bar(ui: &mut egui::Ui, label: &str, pct: f32, value: String) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{label:<4}"))
                .monospace()
                .size(10.0)
                .color(DIM),
        );
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 9.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, Color32::from_rgb(22, 22, 30));
        let mut fill = rect;
        fill.set_width(rect.width() * (pct / 100.0).clamp(0.0, 1.0));
        painter.rect_filled(fill, 2.0, if pct > 85.0 { HOT } else { ACCENT });
    });
    ui.label(egui::RichText::new(value).monospace().size(10.0).color(DIM));
    ui.add_space(4.0);
}

/// A small frameless window-control button; returns true when clicked.
fn win_button(ui: &mut egui::Ui, glyph: &str, color: Color32) -> bool {
    let resp = ui.add(
        egui::Button::new(egui::RichText::new(glyph).monospace().size(14.0).color(color))
            .frame(false)
            .min_size(egui::vec2(22.0, 18.0)),
    );
    if resp.hovered() {
        ui.painter().rect_filled(
            resp.rect.expand(2.0),
            egui::Rounding::same(3.0),
            Color32::from_rgba_unmultiplied(255, 255, 255, 20),
        );
    }
    resp.clicked()
}

/// Linearly interpolate between two colors (`t` in 0..1).
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn mood_color(mood: Mood) -> Color32 {
    match mood {
        Mood::Neutral => Color32::from_rgb(120, 122, 140),
        Mood::Thinking => Color32::from_rgb(150, 185, 240),
        Mood::Happy => Color32::from_rgb(160, 220, 150),
        Mood::Sad => Color32::from_rgb(240, 120, 120),
        Mood::Upset => Color32::from_rgb(250, 100, 100),
        Mood::Watching => Color32::from_rgb(180, 150, 225),
    }
}
