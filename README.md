# gui-term

A small GUI terminal emulator in Rust, themed after *Serial Experiments Lain*.
It runs a real shell and shows a "Lain" avatar in a side panel whose expression
reacts to what your commands do.

## Features

- Full terminal emulation (works with `vim`, `nano`, and other TUIs)
- **Lain reacts to your commands** — happy on success, sad/upset on failure,
  "watching" while you type, via OSC 133 shell integration
- Animated sprites (GIFs) per mood, with an optional MP4/GIF boot splash
- Live system stats (CPU, memory, network, load) from `/proc`
- Custom "Navi"-style window frame with CRT scanlines

## Requirements

- Rust (edition 2024)
- Linux (system stats read from `/proc`)
- `ffmpeg` / `ffprobe` on `PATH` (optional, only for the MP4 boot splash)

## Run

```bash
cargo run --release
```

## Sprites

Drop images in `sprites-backgrounds/`. Each file is matched to a mood by
keywords in its name (e.g. `happy.gif`, `sad.gif`, `upset.gif`, `watching.gif`,
`neutral.gif`). A file named `copeland_os.*` is used as the startup splash and
`copeland_background.png` as the backdrop. `.png`, `.jpg`, and `.gif` are
supported; if a mood has no sprite, a simple placeholder is drawn.

## Keybindings

- `Ctrl+Shift+Q` — quit
- All other keys are forwarded to the shell.
