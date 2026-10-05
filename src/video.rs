use crate::{
    frame::{Frame, read_ppm},
    session::decoder_command,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    io::{BufReader, Read, Write},
    process::{Child, ChildStdin, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub enum Packet {
    Session(u16, u16),
    Media(Vec<u8>),
}

pub fn read_packet(reader: &mut impl Read) -> Result<Option<Packet>> {
    let mut header = [0; 12];
    if reader.read(&mut header[..1])? == 0 {
        return Ok(None);
    }
    reader
        .read_exact(&mut header[1..])
        .context("truncated scrcpy packet header")?;
    if header[0] & 128 != 0 {
        let width = u32::from_be_bytes(header[4..8].try_into().unwrap());
        let height = u32::from_be_bytes(header[8..12].try_into().unwrap());
        ensure!(
            width > 0 && height > 0 && width <= 4096 && height <= 4096,
            "invalid scrcpy capture dimensions"
        );
        return Ok(Some(Packet::Session(width as u16, height as u16)));
    }
    let size = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    ensure!(
        size > 0 && size <= 8 * 1024 * 1024,
        "invalid or oversized scrcpy media packet"
    );
    let mut data = vec![0; size];
    reader
        .read_exact(&mut data)
        .context("truncated scrcpy media packet")?;
    Ok(Some(Packet::Media(data)))
}

// Capture-session metadata appeared in scrcpy 4. A new decoder is needed because
// FFmpeg's image encoder retains its original dimensions across H264 changes.
pub fn decode_stream(
    reader: &mut impl Read,
    ffmpeg: &str,
    on_frame: Arc<dyn Fn(Frame) + Send + Sync>,
) -> Result<()> {
    let mut codec = [0; 4];
    reader.read_exact(&mut codec).context("read scrcpy codec")?;
    ensure!(&codec == b"h264", "Expected an H264 screen stream");
    let mut decoder: Option<Decoder> = None;
    while let Some(packet) = read_packet(reader)? {
        match packet {
            Packet::Session(width, height) => {
                if let Some(previous) = decoder.take() {
                    previous.finish(true)?
                }
                decoder = Some(Decoder::start(
                    ffmpeg,
                    width,
                    height,
                    Arc::clone(&on_frame),
                )?);
            }
            Packet::Media(data) => {
                decoder
                    .as_mut()
                    .context("media arrived before capture-session dimensions")?
                    .input
                    .as_mut()
                    .unwrap()
                    .write_all(&data)
                    .context("feed video to FFmpeg")?;
            }
        }
    }
    if let Some(decoder) = decoder {
        decoder.finish(false)?
    }
    Ok(())
}

struct Decoder {
    process: Child,
    input: Option<ChildStdin>,
    reader: Option<JoinHandle<Result<usize>>>,
    logs: Arc<Mutex<Vec<u8>>>,
}

impl Decoder {
    fn start(
        ffmpeg: &str,
        width: u16,
        height: u16,
        on_frame: Arc<dyn Fn(Frame) + Send + Sync>,
    ) -> Result<Self> {
        let mut process = decoder_command(ffmpeg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start FFmpeg decoder")?;
        let input = process.stdin.take();
        let output = process.stdout.take().unwrap();
        let mut stderr = process.stderr.take().unwrap();
        let logs = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&logs);
        thread::spawn(move || {
            let mut buffer = [0; 1024];
            while let Ok(count) = stderr.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let mut log = collected.lock().unwrap();
                if log.len() < 16384 {
                    log.extend_from_slice(&buffer[..count]);
                }
            }
        });
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut count = 0;
            while let Some(frame) = read_ppm(&mut reader)? {
                ensure!(
                    (frame.width, frame.height) == (width, height),
                    "decoded dimensions disagree with scrcpy capture metadata"
                );
                on_frame(frame);
                count += 1;
            }
            Ok(count)
        });
        Ok(Self {
            process,
            input,
            reader: Some(reader),
            logs,
        })
    }

    fn finish(mut self, allow_empty: bool) -> Result<()> {
        self.input.take();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = self.process.try_wait()? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(2) {
                bail!("FFmpeg did not finish decoding the capture session")
            }
            thread::sleep(Duration::from_millis(5));
        };
        let count = if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("video reader panicked"))??
        } else {
            0
        };
        if count == 0 && allow_empty {
            return Ok(());
        }
        ensure!(
            status.success(),
            "FFmpeg failed: {}",
            String::from_utf8_lossy(&self.logs.lock().unwrap())
        );
        Ok(())
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.process.kill();
        let _ = self.process.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
