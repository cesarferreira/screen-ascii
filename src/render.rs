use crate::{frame::Frame, geometry::Viewport};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Mode {
    Ascii,
    Blocks,
    Graphics,
}
pub fn detailed_lines(
    frame: &Frame,
    view: Viewport,
    ramp: &str,
    color: bool,
    invert: bool,
    mode: Mode,
) -> Vec<String> {
    match mode {
        Mode::Ascii => ascii_detail(frame, view, ramp, color, invert),
        Mode::Blocks => blocks(frame, view, color, invert),
        Mode::Graphics => Vec::new(),
    }
}

fn sample(frame: &Frame, x: usize, y: usize, cols: usize, rows: usize) -> [u8; 3] {
    let x0 = x * frame.width as usize / cols;
    let y0 = y * frame.height as usize / rows;
    let x1 = ((x + 1) * frame.width as usize / cols)
        .max(x0 + 1)
        .min(frame.width as usize);
    let y1 = ((y + 1) * frame.height as usize / rows)
        .max(y0 + 1)
        .min(frame.height as usize);
    let mut sum = [0_u64; 3];
    let mut count = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = (y * frame.width as usize + x) * 3;
            for (channel, total) in sum.iter_mut().enumerate() {
                *total += u64::from(frame.rgb[offset + channel]);
            }
            count += 1;
        }
    }
    sum.map(|value| (value / count) as u8)
}

fn luma(rgb: [u8; 3]) -> u8 {
    ((299 * u32::from(rgb[0]) + 587 * u32::from(rgb[1]) + 114 * u32::from(rgb[2])) / 1000) as u8
}

fn blocks(frame: &Frame, view: Viewport, color: bool, invert: bool) -> Vec<String> {
    (0..usize::from(view.rows))
        .map(|row| {
            let mut line = String::new();
            for col in 0..usize::from(view.cols) {
                let mut top = sample(
                    frame,
                    col,
                    row * 2,
                    view.cols as usize,
                    view.rows as usize * 2,
                );
                let mut bottom = sample(
                    frame,
                    col,
                    row * 2 + 1,
                    view.cols as usize,
                    view.rows as usize * 2,
                );
                if !color {
                    top = [luma(top); 3];
                    bottom = [luma(bottom); 3];
                }
                if invert {
                    top = top.map(|v| 255 - v);
                    bottom = bottom.map(|v| 255 - v);
                }
                write!(
                    line,
                    "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀",
                    top[0], top[1], top[2], bottom[0], bottom[1], bottom[2]
                )
                .unwrap();
            }
            line.push_str("\x1b[39m\x1b[48;2;0;0;0m");
            line
        })
        .collect()
}

fn ascii_detail(
    frame: &Frame,
    view: Viewport,
    ramp: &str,
    color: bool,
    invert: bool,
) -> Vec<String> {
    let ramp: Vec<char> = ramp.chars().collect();
    (0..view.rows as usize)
        .map(|row| {
            let mut line = String::new();
            for col in 0..view.cols as usize {
                let rgb = sample(frame, col, row, view.cols as usize, view.rows as usize);
                let brightness = luma(rgb);
                let mapped = (u16::from(brightness.saturating_sub(32)) * 255 / 223) as usize;
                let mapped = if invert { 255 - mapped } else { mapped };
                let character = ramp[(mapped * (ramp.len() - 1) + 127) / 255];
                if color {
                    let lift = 110_u8.saturating_sub(brightness);
                    let visible = rgb.map(|v| v.saturating_add(lift));
                    write!(
                        line,
                        "\x1b[38;2;{};{};{}m",
                        visible[0], visible[1], visible[2]
                    )
                    .unwrap();
                }
                line.push(character);
            }
            if color {
                line.push_str("\x1b[39m");
            }
            line
        })
        .collect()
}
