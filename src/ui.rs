use anyhow::{Context, Result, ensure};
use ascii_scrcpy::{
    frame::Frame, geometry::Viewport, input::TouchState, protocol, render, session::Session,
};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{
        self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use std::{
    io::{self, IsTerminal, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub struct Options {
    pub mode: render::Mode,
    pub color: bool,
    pub invert: bool,
    pub ramp: String,
    pub aspect: f32,
    pub fps: u16,
}

struct Terminal;
impl Terminal {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            EnableMouseCapture,
            EnableBracketedPaste,
            SetBackgroundColor(Color::Black),
            SetForegroundColor(Color::White),
            Clear(ClearType::All)
        )?;
        Ok(guard)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            EndSynchronizedUpdate,
            ResetColor,
            DisableBracketedPaste,
            DisableMouseCapture,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

pub fn snapshot(
    session: Option<&Session>,
    options: &Options,
    cols: u16,
    rows: u16,
    cancelled: &AtomicBool,
) -> Result<()> {
    let frame = if let Some(session) = session {
        let start = Instant::now();
        loop {
            ensure!(!cancelled.load(Ordering::Relaxed), "Connection cancelled");
            if let Some(frame) = session.latest()?.1 {
                break frame;
            }
            ensure!(
                start.elapsed() < Duration::from_secs(15),
                "No video frames after 15 seconds. {}",
                session.diagnostics()
            );
            thread::sleep(Duration::from_millis(20));
        }
    } else {
        Arc::new(demo_frame(0.0))
    };
    let view = Viewport::fit(
        cols,
        rows.saturating_add(2),
        frame.width,
        frame.height,
        options.aspect,
    );
    let mut out = io::stdout().lock();
    for line in render::detailed_lines(
        &frame,
        view,
        &options.ramp,
        options.color,
        options.invert,
        options.mode,
    ) {
        writeln!(out, "{line}")?
    }
    Ok(())
}

pub fn run(
    mut session: Option<Session>,
    mut options: Options,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "Interactive mode needs a terminal. Use --snapshot for plain output."
    );
    let _terminal = Terminal::enter()?;
    let mut out = io::stdout();
    let mut touch = TouchState::default();
    let start = Instant::now();
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut last_sequence = 0;
    let mut last_size = (0, 0);
    let mut view = None;
    let mut force_draw = true;
    let mut typing: Option<String> = None;
    let interval = Duration::from_secs_f64(1.0 / f64::from(options.fps));
    loop {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let (sequence, frame) = if let Some(session) = &session {
            session.latest()?
        } else {
            let elapsed = start.elapsed().as_secs_f32();
            (
                (elapsed * f32::from(options.fps)) as u64 + 1,
                Some(Arc::new(demo_frame(elapsed))),
            )
        };
        let size = terminal::size()?;
        if size != last_size {
            force_draw = true
        }
        if let Some(frame) = frame {
            if last_size != size
                || view.is_some_and(|view: Viewport| {
                    (view.width, view.height) != (frame.width, frame.height)
                })
            {
                for packet in touch.cancel() {
                    if let Some(session) = &mut session {
                        session.send(&packet)?
                    }
                }
                force_draw = true;
            }
            if force_draw || (sequence != last_sequence && last_draw.elapsed() >= interval) {
                let viewport =
                    Viewport::fit(size.0, size.1, frame.width, frame.height, options.aspect);
                let label = session
                    .as_ref()
                    .map(|session| session.serial.as_str())
                    .unwrap_or("demo");
                draw(
                    &mut out,
                    &frame,
                    viewport,
                    size,
                    &options,
                    label,
                    typing.as_deref(),
                )?;
                view = Some(viewport);
                last_size = size;
                last_sequence = sequence;
                last_draw = Instant::now();
                force_draw = false;
            }
        } else {
            ensure!(
                start.elapsed() < Duration::from_secs(15),
                "No video frames after 15 seconds. {}",
                session
                    .as_ref()
                    .map(Session::diagnostics)
                    .unwrap_or_default()
            );
            if force_draw {
                queue!(
                    out,
                    MoveTo(0, 0),
                    Print("Waiting for Android video…  Ctrl+C to quit")
                )?;
                out.flush()?;
                force_draw = false;
            }
        }
        if !event::poll(Duration::from_millis(8))? {
            continue;
        }
        match event::read()? {
            Event::Mouse(event) => {
                if let Some(view) = view {
                    for packet in touch.handle(event, view) {
                        if let Some(session) = &mut session {
                            session.send(&packet)?
                        }
                    }
                }
            }
            Event::Resize(_, _) => force_draw = true,
            Event::Paste(value) => {
                if let Some(text) = &mut typing {
                    text.extend(value.chars().filter(|ch| !ch.is_control()));
                    force_draw = true
                }
            }
            Event::Key(event) if event.kind != KeyEventKind::Release => {
                if event.code == KeyCode::Char('c')
                    && event.modifiers.contains(KeyModifiers::CONTROL)
                {
                    break;
                }
                if let Some(text) = &mut typing {
                    match event.code {
                        KeyCode::Esc => typing = None,
                        KeyCode::Enter => {
                            if let Some(session) = &mut session {
                                send_text(session, text)?
                            }
                            typing = None;
                        }
                        KeyCode::Backspace => {
                            text.pop();
                        }
                        KeyCode::Char(ch)
                            if !event
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            text.push(ch)
                        }
                        _ => {}
                    }
                    force_draw = true;
                    continue;
                }
                let key = match event.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('t') => {
                        typing = Some(String::new());
                        force_draw = true;
                        None
                    }
                    KeyCode::Char('c') => {
                        options.color = !options.color;
                        force_draw = true;
                        None
                    }
                    KeyCode::Char('a') => {
                        options.mode = match options.mode {
                            render::Mode::Ascii => render::Mode::Blocks,
                            render::Mode::Blocks => render::Mode::Ascii,
                        };
                        force_draw = true;
                        None
                    }
                    KeyCode::Char('i') => {
                        options.invert = !options.invert;
                        force_draw = true;
                        None
                    }
                    KeyCode::Char('h') | KeyCode::Home => Some(3),
                    KeyCode::Char('b') | KeyCode::Esc => Some(4),
                    KeyCode::Char('r') => Some(187),
                    KeyCode::Char('p') => Some(26),
                    KeyCode::Enter => Some(66),
                    KeyCode::Backspace => Some(67),
                    KeyCode::Up => Some(19),
                    KeyCode::Down => Some(20),
                    KeyCode::Left => Some(21),
                    KeyCode::Right => Some(22),
                    _ => None,
                };
                if let (Some(code), Some(session)) = (key, &mut session) {
                    session.keypress(code)?
                }
            }
            _ => {}
        }
    }
    for packet in touch.cancel() {
        if let Some(session) = &mut session {
            let _ = session.send(&packet);
        }
    }
    Ok(())
}

fn send_text(session: &mut Session, mut text: &str) -> Result<()> {
    while !text.is_empty() {
        let packet = protocol::text(text);
        session.send(&packet)?;
        text = &text[packet.len() - 5..];
    }
    Ok(())
}

fn draw(
    out: &mut impl Write,
    frame: &Frame,
    view: Viewport,
    size: (u16, u16),
    options: &Options,
    label: &str,
    typing: Option<&str>,
) -> Result<()> {
    queue!(
        out,
        BeginSynchronizedUpdate,
        ResetColor,
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::White),
        MoveTo(0, 0),
        Clear(ClearType::All)
    )?;
    let header = format!(
        " ascii-scrcpy | {label} | {}×{} | {}×{} chars",
        frame.width, frame.height, view.cols, view.rows
    );
    queue!(
        out,
        Print(header.chars().take(size.0 as usize).collect::<String>())
    )?;
    for (row, line) in render::detailed_lines(
        frame,
        view,
        &options.ramp,
        options.color,
        options.invert,
        options.mode,
    )
    .iter()
    .enumerate()
    {
        queue!(out, MoveTo(view.x, view.y + row as u16), Print(line))?;
    }
    let footer = typing
        .map(|text| format!(" Text: {text}  [Enter sends · Esc cancels]"))
        .unwrap_or_else(|| {
            " click/drag · b Back · h Home · r Recents · t Type · a ASCII/Blocks · c Colour · i Invert · q Quit"
                .to_owned()
        });
    queue!(
        out,
        ResetColor,
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::White),
        MoveTo(0, size.1.saturating_sub(1)),
        Print(footer.chars().take(size.0 as usize).collect::<String>()),
        EndSynchronizedUpdate
    )?;
    out.flush().context("draw terminal frame")
}

fn demo_frame(time: f32) -> Frame {
    let (width, height) = (240_u16, 480_u16);
    let mut rgb = Vec::with_capacity(usize::from(width) * usize::from(height) * 3);
    for y in 0..height {
        for x in 0..width {
            let colour = if !(30..=450).contains(&y) {
                [190, 200, 220]
            } else if (50..100).contains(&y) && (20..220).contains(&x) {
                [255, 255, 255]
            } else if (140..380).contains(&y) && (20..220).contains(&x) {
                let col = (x - 20) / 70;
                let row = (y - 140) / 80;
                if (x - 20) % 70 < 48 && (y - 140) % 80 < 48 {
                    [60 + col as u8 * 70, 70 + row as u8 * 60, 210]
                } else {
                    [15, 20, 32]
                }
            } else {
                let wave = ((f32::from(x) / 15.0 + time * 3.0).sin() * 15.0 + 20.0) as u8;
                [wave, wave + 10, wave + 25]
            };
            rgb.extend_from_slice(&colour);
        }
    }
    Frame { width, height, rgb }
}
