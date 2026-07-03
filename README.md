# gui-term

A GUI terminal emulator in Rust.
It runs a shell and shows a "Lain" avatar in a side panel whose expression
reacts to what your commands do.

Click the picture to view it !!!
[![Watch the demo](https://img.youtube.com/vi/POFEEpMix2g/hqdefault.jpg)](https://www.youtube.com/watch?v=POFEEpMix2g)
## Features

- Full terminal emulation (works with `vim`, `nano`, and other TUIs)
- Lain reacts to your commands happy, sad/upset,
  "watching" while you type
- Animated sprites (GIFs) per mood
- Live system stats (CPU, memory, network, load) from `/proc`

## Requirements

- Rust
- Linux

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

- `Ctrl+Shift+Q` to quit
- All other keys are forwarded to the shell.
