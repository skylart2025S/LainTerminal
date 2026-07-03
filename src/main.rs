mod ansi;
mod app;
mod input;
mod lain;
mod stats;
mod video;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use app::LainTerminal;

const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 120;

fn main() -> eframe::Result<()> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("failed to open pty");

    let mut cmd = CommandBuilder::new("bash");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let _child = pair.slave.spawn_command(cmd).expect("failed to spawn bash");

    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("failed to clone reader");
    let mut writer = pair.master.take_writer().expect("failed to take writer");
    let master = pair.master;

    // GUI -> PTY resize (master is kept alive here).
    let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>();
    thread::spawn(move || {
        while let Ok((rows, cols)) = resize_rx.recv() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    });

    // PTY -> GUI (raw bytes, including escape sequences).
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || out_tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    // GUI -> PTY input.
    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        while let Ok(bytes) = in_rx.recv() {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    // Enable OSC 133 shell integration so command start/exit can drive Lain's mood.
    let init = concat!(
        "PS0=$'\\e]133;C\\a'; ",
        "PROMPT_COMMAND='printf \"\\033]133;D;%s\\007\" \"$?\"'; ",
        "clear\n"
    );
    let _ = in_tx.send(init.as_bytes().to_vec());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 640.0])
            .with_min_inner_size([600.0, 400.0])
            .with_decorations(false)
            .with_title("Lain Terminal"),
        ..Default::default()
    };

    eframe::run_native(
        "Lain Terminal",
        options,
        Box::new(move |_cc| Ok(Box::new(LainTerminal::new(out_rx, in_tx, resize_tx)))),
    )
}
