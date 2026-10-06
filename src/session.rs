use crate::{frame::Frame, protocol};
use anyhow::{Context, Result, bail, ensure};
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Config {
    pub serial: Option<String>,
    pub adb: String,
    pub scrcpy: String,
    pub ffmpeg: String,
    pub server: Option<PathBuf>,
    pub server_version: Option<String>,
    pub max_size: u16,
    pub max_fps: u16,
    pub cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct Frames {
    latest: Option<Arc<Frame>>,
    sequence: u64,
    error: Option<String>,
}

pub struct Session {
    pub serial: String,
    adb: String,
    remote: String,
    forward: Option<String>,
    server: Option<Child>,
    video: Option<TcpStream>,
    control: Option<TcpStream>,
    workers: Vec<JoinHandle<()>>,
    frames: Arc<Mutex<Frames>>,
    logs: Arc<Mutex<Vec<u8>>>,
    cancelled: Arc<AtomicBool>,
}

impl Session {
    pub fn start(config: Config) -> Result<Self> {
        ensure!(
            Command::new(&config.ffmpeg)
                .arg("-version")
                .output()
                .context("FFmpeg is required; install it and put it on PATH")?
                .status
                .success(),
            "FFmpeg could not run"
        );
        let devices = checked(
            Command::new(&config.adb).args(["devices"]),
            "list Android devices",
        )?;
        let serial = select_device(&devices, config.serial.as_deref())?;
        let version = if let Some(version) = config.server_version {
            version
        } else {
            let output = checked(
                Command::new(&config.scrcpy).arg("--version"),
                "detect scrcpy version (install scrcpy or supply --server and --server-version)",
            )?;
            parse_version(&output)?
        };
        validate_version(&version)?;
        let server_path = match config.server {
            Some(path) => path,
            None => find_server(&config.scrcpy)?,
        };
        ensure!(
            server_path.is_file(),
            "scrcpy server not found: {}",
            server_path.display()
        );
        let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u32 & 0x7fffffff;
        let remote = format!("/data/local/tmp/screen-ascii-{id:08x}.jar");
        let mut session = Self {
            serial,
            adb: config.adb,
            remote,
            forward: None,
            server: None,
            video: None,
            control: None,
            workers: vec![],
            frames: Arc::new(Mutex::new(Frames::default())),
            logs: Arc::new(Mutex::new(Vec::new())),
            cancelled: config.cancelled,
        };
        checked(
            session
                .adb()
                .arg("push")
                .arg(server_path)
                .arg(&session.remote),
            "push scrcpy server",
        )?;
        let socket = format!("localabstract:scrcpy_{id:08x}");
        let port = checked(
            session.adb().args(["forward", "tcp:0", &socket]),
            "forward scrcpy socket",
        )?;
        let port: u16 = port
            .trim()
            .parse()
            .context("adb did not return a TCP port")?;
        session.forward = Some(format!("tcp:{port}"));
        session.server = Some(
            session
                .adb()
                .args([
                    "shell",
                    &format!("CLASSPATH={}", session.remote),
                    "app_process",
                    "/",
                    "com.genymobile.scrcpy.Server",
                    &version,
                    &format!("scid={id:08x}"),
                    "tunnel_forward=true",
                    "audio=false",
                    "control=true",
                    "video_codec=h264",
                    "raw_stream=true",
                    "send_dummy_byte=true",
                    "send_stream_meta=true",
                    "send_frame_meta=true",
                    "cleanup=true",
                    "clipboard_autosync=false",
                    &format!("max_size={}", config.max_size),
                    &format!("max_fps={}", config.max_fps),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("start Android scrcpy server")?,
        );
        let server = session.server.as_mut().unwrap();
        collect_logs(server.stdout.take().unwrap(), Arc::clone(&session.logs));
        collect_logs(server.stderr.take().unwrap(), Arc::clone(&session.logs));
        let video = session
            .connect_video(port)
            .with_context(|| session.diagnostics())?;
        session.video = Some(video.try_clone()?);
        let control = TcpStream::connect(("127.0.0.1", port)).context("connect control channel")?;
        control.set_nodelay(true)?;
        control.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut receiver = control.try_clone()?;
        session.workers.push(thread::spawn(move || {
            let _ = io::copy(&mut receiver, &mut io::sink());
        }));
        session.control = Some(control);

        let mut video = video;
        let frames = Arc::clone(&session.frames);
        let on_frame_state = Arc::clone(&frames);
        let ffmpeg = config.ffmpeg;
        session.workers.push(thread::spawn(move || {
            let result = crate::video::decode_stream(
                &mut video,
                &ffmpeg,
                Arc::new(move |frame| {
                    let mut state = on_frame_state.lock().unwrap();
                    state.latest = Some(Arc::new(frame));
                    state.sequence += 1;
                }),
            );
            frames.lock().unwrap().error = Some(match result {
                Ok(()) => "Video stream ended; the Android device may have disconnected".to_owned(),
                Err(error) => format!("Video decoding failed: {error:#}"),
            });
        }));
        Ok(session)
    }

    fn adb(&self) -> Command {
        let mut command = Command::new(&self.adb);
        command.args(["-s", &self.serial]);
        command
    }

    fn connect_video(&mut self, port: u16) -> Result<TcpStream> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            ensure!(
                !self.cancelled.load(Ordering::Relaxed),
                "Connection cancelled"
            );
            if let Some(status) = self.server.as_mut().unwrap().try_wait()? {
                bail!("Android server exited ({status})");
            }
            if let Ok(mut socket) = TcpStream::connect(("127.0.0.1", port)) {
                socket.set_read_timeout(Some(Duration::from_millis(250)))?;
                let mut dummy = [255];
                if socket.read_exact(&mut dummy).is_ok() && dummy[0] == 0 {
                    socket.set_read_timeout(None)?;
                    return Ok(socket);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        bail!("Timed out waiting for the Android video socket");
    }

    pub fn latest(&self) -> Result<(u64, Option<Arc<Frame>>)> {
        let state = self.frames.lock().unwrap();
        if let Some(error) = &state.error {
            bail!("{error}\n{}", self.diagnostics());
        }
        Ok((state.sequence, state.latest.clone()))
    }

    pub fn send(&mut self, packet: &[u8]) -> Result<()> {
        self.control
            .as_mut()
            .context("no control connection")?
            .write_all(packet)
            .context("send Android input")
    }

    pub fn keypress(&mut self, code: u32) -> Result<()> {
        self.send(&protocol::key(0, code))?;
        self.send(&protocol::key(1, code))
    }

    pub fn diagnostics(&self) -> String {
        String::from_utf8_lossy(&self.logs.lock().unwrap())
            .trim()
            .to_owned()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(socket) = &self.video {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(socket) = &self.control {
            let _ = socket.shutdown(Shutdown::Both);
        }
        for child in [&mut self.server].into_iter().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        if let Some(forward) = &self.forward {
            let _ = self.adb().args(["forward", "--remove", forward]).output();
        }
        let _ = self
            .adb()
            .args(["shell", "rm", "-f", &self.remote])
            .output();
    }
}

fn collect_logs(mut reader: impl Read + Send + 'static, logs: Arc<Mutex<Vec<u8>>>) {
    thread::spawn(move || {
        let mut buffer = [0; 1024];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let mut log = logs.lock().unwrap();
            log.extend_from_slice(&buffer[..count]);
            if log.len() > 16384 {
                let end = log.len() - 16384;
                log.drain(..end);
            }
        }
    });
}

fn checked(command: &mut Command, action: &str) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("Could not {action}"))?;
    ensure!(
        output.status.success(),
        "Could not {action}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn select_device(output: &str, requested: Option<&str>) -> Result<String> {
    let devices: Vec<_> = output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            (fields.next()? == "device").then_some(serial)
        })
        .collect();
    if let Some(requested) = requested {
        ensure!(
            devices.contains(&requested),
            "Device {requested} is unavailable. Check adb devices and approve USB debugging on the device."
        );
        return Ok(requested.to_owned());
    }
    match devices.as_slice() {
        [serial] => Ok((*serial).to_owned()),
        [] => bail!(
            "No authorized Android device. Connect one, enable USB debugging, and approve the connection; check adb devices."
        ),
        _ => bail!(
            "Multiple Android devices connected: {}. Select one with --serial.",
            devices.join(", ")
        ),
    }
}

pub fn parse_version(output: &str) -> Result<String> {
    let version = output
        .lines()
        .find_map(|line| {
            line.strip_prefix("scrcpy ")
                .and_then(|line| line.split_whitespace().next())
        })
        .context("Could not detect scrcpy version; supply --server-version")?;
    validate_version(version)?;
    Ok(version.to_owned())
}

fn validate_version(version: &str) -> Result<()> {
    ensure!(
        version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        "Expected a numeric scrcpy server version, such as 5.0"
    );
    let major: u8 = version.split('.').next().unwrap().parse()?;
    ensure!(
        (4..=5).contains(&major),
        "Supported scrcpy server versions: 4.x and 5.x (tested with 5.0)"
    );
    Ok(())
}

fn find_server(scrcpy: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SCRCPY_SERVER_PATH") {
        return Ok(path.into());
    }
    let binary = if Path::new(scrcpy).components().count() > 1 {
        Some(PathBuf::from(scrcpy))
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(scrcpy))
                .find(|path| path.is_file())
        })
    };
    let mut candidates = vec![];
    if let Some(binary) = binary {
        let binary = binary.canonicalize().unwrap_or(binary);
        if let Some(parent) = binary.parent() {
            candidates.push(parent.join("scrcpy-server"));
            candidates.push(parent.join("../share/scrcpy/scrcpy-server"));
        }
    }
    candidates.extend(
        [
            "/opt/homebrew/share/scrcpy/scrcpy-server",
            "/usr/local/share/scrcpy/scrcpy-server",
            "/usr/share/scrcpy/scrcpy-server",
        ]
        .map(PathBuf::from),
    );
    candidates.into_iter().find(|path| path.is_file()).context("Could not locate scrcpy-server. Supply --server PATH (and --server-version if scrcpy is not installed).")
}

pub fn decoder_command(ffmpeg: &str) -> Command {
    let mut command = Command::new(ffmpeg);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-threads",
        "1",
        "-flags",
        "low_delay",
        "-probesize",
        "32",
        "-analyzeduration",
        "0",
        "-f",
        "h264",
        "-i",
        "pipe:0",
        "-an",
        "-threads",
        "1",
        "-fps_mode",
        "passthrough",
        "-f",
        "image2pipe",
        "-c:v",
        "ppm",
        "pipe:1",
    ]);
    command
}
