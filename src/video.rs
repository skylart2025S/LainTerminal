//! Minimal video playback: `ffmpeg` decodes to raw RGBA (`-re`, downscaled) on a
//! background thread and the UI uploads the newest frame each tick. No audio.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui::{self, Color32, Pos2, Rect};

/// Max decoded height; width scales to preserve aspect ratio.
const TARGET_HEIGHT: u32 = 720;

pub struct VideoPlayer {
    rx: Receiver<Vec<u8>>,
    width: usize,
    height: usize,
    child: Child,
    texture: Option<egui::TextureHandle>,
}

impl VideoPlayer {
    /// Start decoding `path`, or `None` if it's missing or ffmpeg/ffprobe fails.
    pub fn start(path: &str) -> Option<Self> {
        if !std::path::Path::new(path).exists() {
            return None;
        }

        let (src_w, src_h) = probe_dimensions(path)?;
        if src_w == 0 || src_h == 0 {
            return None;
        }

        // Scale to TARGET_HEIGHT, keeping aspect, with even dimensions (h.264/rgba).
        let height = src_h.min(TARGET_HEIGHT);
        let width = (src_w * height / src_h) & !1;
        let height = height & !1;
        let frame_size = width as usize * height as usize * 4;

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-re",
                "-i",
                path,
                "-an",
                "-vf",
                &format!("scale={width}:{height}"),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let mut stdout = child.stdout.take()?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        thread::spawn(move || {
            let mut buf = vec![0u8; frame_size];
            // Ends on EOF/pipe close or when the receiver is dropped.
            while stdout.read_exact(&mut buf).is_ok() {
                if tx.send(buf.clone()).is_err() {
                    break;
                }
            }
        });

        Some(Self {
            rx,
            width: width as usize,
            height: height as usize,
            child,
            texture: None,
        })
    }

    /// Draw the newest decoded frame (dropping backlog) as a cover-fit fill.
    pub fn draw(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        let mut latest = None;
        while let Ok(frame) = self.rx.try_recv() {
            latest = Some(frame);
        }

        if let Some(frame) = latest {
            let image = egui::ColorImage::from_rgba_unmultiplied([self.width, self.height], &frame);
            match &mut self.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ctx.load_texture("boot_video", image, egui::TextureOptions::LINEAR));
                }
            }
        }

        let Some(texture) = &self.texture else {
            return;
        };
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
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Query the video's `(width, height)` in pixels via `ffprobe`.
fn probe_dimensions(path: &str) -> Option<(u32, u32)> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
            path,
        ])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.trim().lines().next()?;
    let (w, h) = line.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}
