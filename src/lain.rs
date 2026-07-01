//! Rendering for the "Lain" avatar.
//!
//! [`Avatar`] loads sprite images (one per [`Mood`]) from a directory and draws
//! the one matching the current mood. If a sprite is missing it falls back to a
//! simple placeholder, so the app always shows *something*.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::Path;

use eframe::egui::{self, Color32, Pos2, Rect};
use image::AnimationDecoder;

use crate::ansi::Mood;

/// One frame of an animated sprite plus how long to show it.
struct Frame {
    texture: egui::TextureHandle,
    delay: f32,
}

/// A sprite is either a single still image or an animated sequence (GIF).
enum Sprite {
    Static(egui::TextureHandle),
    Animated { frames: Vec<Frame>, total: f32 },
}

impl Sprite {
    fn is_animated(&self) -> bool {
        matches!(self, Sprite::Animated { .. })
    }

    /// The texture to display at the given elapsed `time` (seconds).
    fn texture(&self, time: f32) -> &egui::TextureHandle {
        match self {
            Sprite::Static(t) => t,
            Sprite::Animated { frames, total } => {
                if frames.len() <= 1 || *total <= 0.0 {
                    return &frames[0].texture;
                }
                let mut t = time.rem_euclid(*total);
                for frame in frames {
                    if t < frame.delay {
                        return &frame.texture;
                    }
                    t -= frame.delay;
                }
                &frames.last().unwrap().texture
            }
        }
    }
}

/// Loaded sprites keyed by mood, with a painted fallback.
#[derive(Default)]
pub struct Avatar {
    sprites: HashMap<Mood, Sprite>,
    /// Optional boot/splash animation shown once at startup.
    boot: Option<Sprite>,
    loaded: bool,
}

impl Avatar {
    /// Scan `dir` for image files and map each to a mood by keyword in its
    /// filename. Safe to call once; subsequent calls are no-ops.
    ///
    /// When `load_boot` is false, the (potentially large) startup splash GIF is
    /// skipped — used when a video splash supersedes it.
    pub fn load(&mut self, ctx: &egui::Context, dir: impl AsRef<Path>, load_boot: bool) {
        if self.loaded {
            return;
        }
        self.loaded = true;

        let dir = dir.as_ref();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            let is_gif = ext == "gif";
            if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif") {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            // The startup splash animation (e.g. "copeland_os").
            if stem.contains("copeland") || stem.contains("boot") || stem.contains("splash") {
                if load_boot && self.boot.is_none() {
                    self.boot = if is_gif {
                        load_animated(ctx, &path)
                    } else {
                        load_static(ctx, &path)
                    };
                }
                continue;
            }

            let Some(mood) = mood_for_name(&stem) else {
                continue;
            };

            // An animated sprite always wins; otherwise keep the first match.
            match self.sprites.get(&mood) {
                Some(existing) if existing.is_animated() || !is_gif => continue,
                _ => {}
            }

            let sprite = if is_gif {
                load_animated(ctx, &path)
            } else {
                load_static(ctx, &path)
            };
            if let Some(sprite) = sprite {
                self.sprites.insert(mood, sprite);
            }
        }
    }

    /// Whether a startup splash animation was loaded.
    pub fn has_boot(&self) -> bool {
        self.boot.is_some()
    }

    /// Draw the startup splash filling `rect`. No-op if none was loaded.
    pub fn draw_boot(&self, painter: &egui::Painter, rect: Rect, time: f32) {
        if let Some(sprite) = &self.boot {
            draw_cover(painter, rect, sprite.texture(time));
        }
    }

    /// Draw the avatar for `mood` as a full-bleed background filling `rect`
    /// (cover fit, cropping overflow). Falls back to a simple placeholder.
    pub fn draw_background(&self, painter: &egui::Painter, rect: Rect, mood: Mood, time: f32) {
        // Prefer the mood's own sprite, then fall back to the Neutral sprite
        // (e.g. no dedicated image), then to the placeholder.
        let sprite = self
            .sprites
            .get(&mood)
            .or_else(|| self.sprites.get(&Mood::Neutral));

        match sprite {
            Some(sprite) => draw_cover(painter, rect, sprite.texture(time)),
            None => draw_placeholder(painter, rect, mood),
        }
    }
}

/// Match a lowercased filename stem to a mood via keywords, so custom names
/// still work (e.g. "smile", "wired", "close_the_world").
fn mood_for_name(stem: &str) -> Option<Mood> {
    const HAPPY: &[&str] = &["happy", "smile", "joy", "love", "glad", "content"];
    const UPSET: &[&str] = &["upset", "angry", "mad", "annoyed", "frustrat", "scared"];
    const SAD: &[&str] = &["sad", "cry", "error", "close", "fail"];
    const WATCHING: &[&str] = &["watch", "listen", "stare", "observe"];
    const THINKING: &[&str] = &[
        "think", "connect", "load", "wired", "confus", "curious", "process", "run",
    ];
    const NEUTRAL: &[&str] = &["neutral", "idle", "normal", "default", "blank", "present", "calm"];

    let has = |kws: &[&str]| kws.iter().any(|k| stem.contains(k));

    // Check Upset before Sad so alternate-failure names win their own slot.
    if has(HAPPY) {
        Some(Mood::Happy)
    } else if has(UPSET) {
        Some(Mood::Upset)
    } else if has(SAD) {
        Some(Mood::Sad)
    } else if has(WATCHING) {
        Some(Mood::Watching)
    } else if has(THINKING) {
        Some(Mood::Thinking)
    } else if has(NEUTRAL) {
        Some(Mood::Neutral)
    } else {
        None
    }
}

/// Load a single still image as a texture (e.g. a background). Returns `None`
/// if the file is missing or can't be decoded.
pub fn load_texture(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    match load_static(ctx, Path::new(path)) {
        Some(Sprite::Static(texture)) => Some(texture),
        _ => None,
    }
}

/// Draw a texture filling `rect` (cover fit: fills the area, cropping excess).
pub fn draw_cover(painter: &egui::Painter, rect: Rect, texture: &egui::TextureHandle) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).max(rect.height() / size.y);
    let img_rect = Rect::from_center_size(rect.center(), size * scale);
    painter.with_clip_rect(rect).image(
        texture.id(),
        img_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn load_static(ctx: &egui::Context, path: &Path) -> Option<Sprite> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("lain");
    Some(Sprite::Static(upload(ctx, name, &img)))
}

fn load_animated(ctx: &egui::Context, path: &Path) -> Option<Sprite> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file)).ok()?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("lain");

    let mut frames = Vec::new();
    let mut total = 0.0;
    for (i, frame) in decoder.into_frames().flatten().enumerate() {
        let (num, den) = frame.delay().numer_denom_ms();
        // Clamp odd/zero delays to something sane (~10 fps default).
        let delay = if den == 0 { 100.0 } else { num as f32 / den as f32 };
        let delay = (delay / 1000.0).clamp(0.02, 1.0);
        let buffer = frame.into_buffer();
        let texture = upload(ctx, &format!("{name}_{i}"), &buffer);
        frames.push(Frame { texture, delay });
        total += delay;
    }

    if frames.is_empty() {
        return None;
    }
    Some(Sprite::Animated { frames, total })
}

fn upload(ctx: &egui::Context, name: &str, img: &image::RgbaImage) -> egui::TextureHandle {
    let size = [img.width() as usize, img.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_flat_samples().as_slice());
    ctx.load_texture(format!("lain_{name}"), color_image, egui::TextureOptions::LINEAR)
}

/// Minimal fallback drawn when a mood has no sprite: a mood-tinted disc on a
/// dark panel.
fn draw_placeholder(painter: &egui::Painter, rect: Rect, mood: Mood) {
    painter.rect_filled(rect, egui::Rounding::same(6.0), Color32::from_rgb(16, 16, 22));
    let color = match mood {
        Mood::Neutral => Color32::from_rgb(120, 122, 140),
        Mood::Thinking => Color32::from_rgb(120, 160, 220),
        Mood::Happy => Color32::from_rgb(140, 200, 130),
        Mood::Sad => Color32::from_rgb(200, 90, 90),
        Mood::Upset => Color32::from_rgb(220, 70, 70),
        Mood::Watching => Color32::from_rgb(170, 140, 210),
    };
    let r = rect.width().min(rect.height()) * 0.28;
    painter.circle_filled(rect.center(), r, color);
}
