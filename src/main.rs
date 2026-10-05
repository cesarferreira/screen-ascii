mod ui;
use anyhow::{Context, Result, ensure};
use ascii_scrcpy::session::{Config, Session};
use clap::Parser;
use std::{
    io::IsTerminal,
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Parser, Debug)]
#[command(
    version,
    about,
    after_help = "Controls: click to tap, drag to swipe, wheel to scroll.\nb/Esc: Back · h: Home · r: Recents · p: Power · t: type text\na: switch ASCII/blocks · c: toggle colour · i: invert · q/Ctrl+C: quit\nText mode: Enter sends text; Esc cancels. Android requires USB debugging."
)]
struct Cli {
    /// Select an Android device by its adb serial
    #[arg(short, long)]
    serial: Option<String>,
    /// Show connected adb devices and exit
    #[arg(long)]
    list_devices: bool,
    /// Render a synthetic phone screen without external tools
    #[arg(long, conflicts_with_all = ["serial", "list_devices"])]
    demo: bool,
    /// Print one character frame and exit (works without a TTY)
    #[arg(long)]
    snapshot: bool,
    /// Use monochrome rendering (ASCII output contains no ANSI colours)
    #[arg(long)]
    no_color: bool,
    /// Screen renderer: detailed coloured blocks or literal ASCII
    #[arg(long, value_enum, default_value_t = ascii_scrcpy::render::Mode::Blocks)]
    render: ascii_scrcpy::render::Mode,
    /// Reverse the brightness-to-character mapping
    #[arg(long)]
    invert: bool,
    /// ASCII mode characters from dark to bright (printable ASCII only)
    #[arg(long, default_value = " .:-=+*#%@")]
    ramp: String,
    /// Terminal character width divided by height
    #[arg(long, default_value_t = 0.5)]
    char_aspect: f32,
    /// Limit captured screen's longest edge in pixels
    #[arg(short = 'm', long, default_value_t = 1024, value_parser = clap::value_parser!(u16).range(64..=4096))]
    max_size: u16,
    /// Maximum capture and display frame rate
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=120))]
    max_fps: u16,
    /// Snapshot canvas width in characters
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u16).range(1..=1000))]
    cols: u16,
    /// Snapshot canvas height in characters
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=1000))]
    rows: u16,
    /// Path to scrcpy-server (otherwise detected from installed scrcpy)
    #[arg(long)]
    server: Option<PathBuf>,
    /// Exact server version (otherwise detected using scrcpy --version)
    #[arg(long)]
    server_version: Option<String>,
    /// adb executable
    #[arg(long, default_value = "adb")]
    adb: String,
    /// scrcpy executable, used for version and server discovery
    #[arg(long, default_value = "scrcpy")]
    scrcpy: String,
    /// FFmpeg executable
    #[arg(long, default_value = "ffmpeg")]
    ffmpeg: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    ensure!(
        cli.char_aspect.is_finite() && (0.1..=2.0).contains(&cli.char_aspect),
        "--char-aspect must be between 0.1 and 2.0"
    );
    ensure!(
        !cli.ramp.is_empty() && cli.ramp.bytes().all(|byte| (32..=126).contains(&byte)),
        "--ramp must contain printable ASCII characters"
    );
    if cli.list_devices {
        let status = Command::new(cli.adb)
            .args(["devices", "-l"])
            .status()
            .context("run adb devices")?;
        ensure!(status.success(), "adb devices failed");
        return Ok(());
    }
    if !cli.snapshot {
        ensure!(
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            "Interactive mode needs a terminal. Use --snapshot for plain output."
        );
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&cancelled))?;
    }
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&cancelled))?;
    let session = if cli.demo {
        None
    } else {
        Some(Session::start(Config {
            serial: cli.serial,
            adb: cli.adb,
            scrcpy: cli.scrcpy,
            ffmpeg: cli.ffmpeg,
            server: cli.server,
            server_version: cli.server_version,
            max_size: cli.max_size,
            max_fps: cli.max_fps,
            cancelled: Arc::clone(&cancelled),
        })?)
    };
    let options = ui::Options {
        mode: cli.render,
        color: !cli.no_color,
        invert: cli.invert,
        ramp: cli.ramp,
        aspect: cli.char_aspect,
        fps: cli.max_fps,
    };
    if cli.snapshot {
        ui::snapshot(session.as_ref(), &options, cli.cols, cli.rows, &cancelled)
    } else {
        ui::run(session, options, cancelled)
    }
}
