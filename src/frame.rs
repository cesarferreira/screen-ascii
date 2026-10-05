use anyhow::{Context, Result, bail, ensure};
use std::io::BufRead;

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub rgb: Vec<u8>,
}

// PPM keeps each frame's dimensions beside its pixels, including after rotation.
pub fn read_ppm(reader: &mut impl BufRead) -> Result<Option<Frame>> {
    let Some(magic) = token(reader)? else {
        return Ok(None);
    };
    ensure!(magic == "P6", "FFmpeg returned a non-binary PPM frame");
    let width: u16 = token(reader)?.context("missing frame width")?.parse()?;
    let height: u16 = token(reader)?.context("missing frame height")?.parse()?;
    let depth = token(reader)?.context("missing colour depth")?;
    ensure!(depth == "255", "unsupported PPM colour depth");
    let size = usize::from(width) * usize::from(height) * 3;
    ensure!(
        width > 0 && height > 0 && size <= 64 * 1024 * 1024,
        "invalid or oversized video frame"
    );
    let mut rgb = vec![0; size];
    reader
        .read_exact(&mut rgb)
        .context("truncated video frame")?;
    Ok(Some(Frame { width, height, rgb }))
}

fn token(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut value = Vec::new();
    loop {
        let mut byte = [0];
        if reader.read(&mut byte)? == 0 {
            if value.is_empty() {
                return Ok(None);
            }
            bail!("truncated PPM header");
        }
        if byte[0] == b'#' && value.is_empty() {
            let mut comment = Vec::new();
            std::io::Read::take(&mut *reader, 4096).read_until(b'\n', &mut comment)?;
            ensure!(comment.last() == Some(&b'\n'), "oversized PPM comment");
            continue;
        }
        if byte[0].is_ascii_whitespace() {
            if !value.is_empty() {
                return Ok(Some(String::from_utf8(value)?));
            }
        } else {
            value.push(byte[0]);
            ensure!(value.len() <= 32, "oversized PPM header token");
        }
    }
}
