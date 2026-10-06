//! Pixel rendering through the Kitty terminal graphics protocol.
use crate::{frame::Frame, geometry::Viewport};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::Write;

pub const DELETE: &str = "\x1b_Ga=d,d=I,i=31,q=2;\x1b\\";

pub fn write_frame(
    out: &mut impl Write,
    frame: &Frame,
    view: Viewport,
    color: bool,
    invert: bool,
) -> Result<()> {
    let mut pixels = frame.rgb.clone();
    for pixel in pixels.chunks_exact_mut(3) {
        if !color {
            let gray = ((299 * u32::from(pixel[0])
                + 587 * u32::from(pixel[1])
                + 114 * u32::from(pixel[2]))
                / 1000) as u8;
            pixel.fill(gray);
        }
        if invert {
            for channel in pixel {
                *channel = 255 - *channel;
            }
        }
    }
    let mut image = Vec::new();
    {
        let mut encoder =
            png::Encoder::new(&mut image, u32::from(frame.width), u32::from(frame.height));
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.write_header()?.write_image_data(&pixels)?;
    }
    let encoded = STANDARD.encode(image);
    let chunks = encoded.as_bytes().chunks(4096);
    let count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        if index == 0 {
            write!(
                out,
                "\x1b_Ga=T,f=100,t=d,i=31,p=1,q=2,C=1,c={},r={},",
                view.cols, view.rows
            )?;
        } else {
            write!(out, "\x1b_Gq=2,")?;
        }
        write!(out, "m={};", u8::from(index + 1 < count))?;
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// Probe before crossterm starts reading events, so protocol replies cannot become keys.
#[cfg(unix)]
pub fn check_terminal(cancelled: &std::sync::atomic::AtomicBool) -> Result<()> {
    use std::{
        io::{self},
        os::fd::AsRawFd,
        time::{Duration, Instant},
    };
    crossterm::terminal::enable_raw_mode()?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _restore = Restore;
    let mut output = io::stdout().lock();
    output.write_all(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c")?;
    output.flush()?;
    let deadline = Instant::now() + Duration::from_millis(700);
    let input = io::stdin().lock();
    let mut response = Vec::new();
    while Instant::now() < deadline && response.len() < 4096 {
        ensure!(
            !cancelled.load(std::sync::atomic::Ordering::Relaxed),
            "Connection cancelled"
        );
        let mut descriptor = libc::pollfd {
            fd: input.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // The descriptor belongs to the locked stdin and remains valid throughout poll.
        let available = unsafe { libc::poll(&mut descriptor, 1, 20) };
        if available <= 0 {
            continue;
        }
        let mut bytes = [0_u8; 512];
        // Read the descriptor directly: std::io buffering would hide unread bytes from poll.
        let count =
            unsafe { libc::read(input.as_raw_fd(), bytes.as_mut_ptr().cast(), bytes.len()) };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if count == 0 {
            break;
        }
        response.extend_from_slice(&bytes[..count as usize]);
        // Wait for device attributes too, draining the entire probe's replies.
        if response.last() == Some(&b'c') && response.windows(3).any(|w| w == b"\x1b[?") {
            break;
        }
    }
    ensure!(
        response
            .windows(b"\x1b_Gi=31;OK\x1b\\".len())
            .any(|w| w == b"\x1b_Gi=31;OK\x1b\\"),
        "This terminal did not confirm Kitty graphics support. Run --render graphics in Kitty or a compatible terminal, or use --render blocks."
    );
    Ok(())
}

#[cfg(not(unix))]
pub fn check_terminal(_cancelled: &std::sync::atomic::AtomicBool) -> Result<()> {
    anyhow::bail!(
        "Graphics support detection currently requires Unix. Use --render blocks on this platform."
    )
}
