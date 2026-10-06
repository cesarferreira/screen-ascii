<div align="center">
  <h1>screen-ascii</h1>

  <p><strong>Control Android devices through a live ASCII terminal</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
    <img alt="Edition" src="https://img.shields.io/badge/edition-2024-blue">
  </p>

  <p>
    <a href="#install">Install</a>
    &nbsp;·&nbsp;
    <a href="#quickstart">Quickstart</a>
  </p>
</div>

---

## Install

Requires [Rust](https://rustup.rs) **1.85+**, `adb`, `scrcpy` **4.x or 5.x**, and `ffmpeg` on your `PATH`. Tested on macOS with scrcpy **5.0** and an Android emulator. A terminal with mouse reporting and truecolour support gives the best experience. Android must have USB debugging enabled and the computer authorized.

On macOS:

```bash
brew install scrcpy ffmpeg
```

Install from [crates.io](https://crates.io/crates/screen-ascii):

```bash
cargo install screen-ascii --locked
```

<a id="quickstart"></a>
## Quickstart

```bash
# Mirror the only connected Android device
screen-ascii

# Choose a device when several are connected
screen-ascii --list-devices
screen-ascii --serial emulator-5554

# Literal ASCII, or a smaller capture at 15 FPS
screen-ascii --render ascii --no-color
screen-ascii --max-size 640 --max-fps 15

# Full pixel graphics for readable phone text (Kitty-compatible terminal)
screen-ascii --render graphics

# Try the renderer without a phone
screen-ascii --demo

# Print one frame without entering the interactive terminal
screen-ascii --snapshot --render ascii --no-color --cols 120 --rows 50
```

### Controls

| Input | Action |
| --- | --- |
| Left click | Tap at that location |
| Left drag | Swipe; holding the mouse down also supports long presses |
| Mouse wheel | Scroll |
| Right click / `b` / Escape | Back |
| Middle click / `h` / Home | Home |
| `r` | Recent apps |
| `p` | Power button |
| Arrow keys | Android directional keys |
| Enter / Backspace | Android Enter / Delete |
| `t` | Enter text mode; type or paste, then Enter to send; Escape cancels |
| `a` | Cycle graphics (when enabled), ASCII, and blocks |
| `c` | Toggle colour (blocks use grayscale when disabled) |
| `i` | Invert character brightness |
| `q` / Ctrl+C | Quit |

Use `--render graphics` in a terminal supporting the [Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) for full pixel frames and readable text. The app probes support before connecting and reports an error if the terminal does not respond. Graphics uses lossless PNG transmission, fits the image to the terminal, and retains click/drag controls. Capture defaults to a 2048-pixel longest edge; use `--max-size 4096` for more source detail if needed. Maximize the window for more displayed pixels. Graphics requires an interactive Unix terminal and cannot be used with `--snapshot`; terminal multiplexers may need graphics passthrough support.

The default renderer uses coloured half-block characters, preserving two independent pixels per terminal cell. Use `--render ascii` for literal ASCII; it suppresses dark background speckle and lifts dim foreground colours. Press `a` to switch modes during a session.

Clicks map to the centre of each character cell. The view preserves the phone's proportions and responds to terminal resizing and device rotation. Reduce the terminal font size for more detail. `--char-aspect` adjusts character proportions (default `0.5`); `--ramp` sets the ASCII characters from dark to bright.

Text mode uses scrcpy's text injection. Android input-method limitations apply: arbitrary Unicode, emoji, and composing input are not guaranteed. This is a visual conversion of the phone's pixels; ASCII and block modes cannot preserve small UI text as readable text; graphics displays the actual pixels without OCR. Audio, clipboard synchronization, and multitouch are outside this first version.

### Server discovery

The client uses `scrcpy --version` and finds the matching server beside the installed scrcpy executable or in common Homebrew/Linux locations. `SCRCPY_SERVER_PATH` is supported. For a custom install:

```bash
screen-ascii --server /path/to/scrcpy-server --server-version 5.0
```

`--adb`, `--scrcpy`, and `--ffmpeg` accept executable paths. ADB-over-network devices work when already listed by `adb devices`. The server version must match the supplied server file exactly. The internal scrcpy protocol may change; 4.x and 5.x are supported, with 5.0 verified locally.

### How it works

```text
Android screen → scrcpy H264 stream → FFmpeg → RGB frames → ASCII terminal
Android touch  ← scrcpy control socket ← terminal mouse coordinates
```

The renderer uses the same brightness mapping, per-cell RGB colour, and character-aspect approach as [ascii-cam](https://github.com/cesarferreira/ascii-cam). It averages the pixels in each character cell. [scrcpy](https://github.com/Genymobile/scrcpy) supplies screen capture and Android input. Capture-session metadata triggers a decoder restart when the phone rotates, preserving the geometry used for touch messages.

The session starts its own server and ADB tunnel. Normal exit, Ctrl+C, SIGTERM, and (on Unix) SIGHUP restore the terminal and remove those session resources.

## License

MIT
