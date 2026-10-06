use screen_ascii::{frame::read_ppm, geometry::Viewport, protocol, render};
use std::io::Cursor;

#[test]
fn portrait_view_is_centered_with_character_aspect_and_rejects_margins() {
    let view = Viewport::fit(120, 42, 1080, 1920, 0.5);
    assert_eq!((view.x, view.y, view.cols, view.rows), (37, 1, 45, 40));
    assert_eq!(view.point(37, 1), Some((12, 24)));
    assert_eq!(view.point(81, 40), Some((1068, 1896)));
    assert_eq!(view.point(36, 1), None);
    assert_eq!(view.point(37, 0), None);
    assert_eq!(view.point(82, 1), None);
}

#[test]
fn landscape_and_small_terminals_preserve_valid_coordinates() {
    let view = Viewport::fit(80, 24, 1920, 1080, 0.5);
    assert_eq!((view.cols, view.rows), (78, 22));
    let tiny = Viewport::fit(1, 1, 1080, 1920, 0.5);
    assert_eq!(tiny.point(0, 0), Some((540, 960)));
}

#[test]
fn touch_packet_matches_scrcpy_wire_format() {
    assert_eq!(
        protocol::touch(0, (100, 200), (1080, 1920)),
        vec![
            2, 0, 255, 255, 255, 255, 255, 255, 255, 254, 0, 0, 0, 100, 0, 0, 0, 200, 4, 56, 7,
            128, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    assert_eq!(
        &protocol::touch(1, (100, 200), (1080, 1920))[22..24],
        &[0, 0]
    );
}

#[test]
fn navigation_and_utf8_text_use_correct_lengths_and_byte_order() {
    assert_eq!(
        protocol::key(0, 3),
        vec![0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(protocol::text("hé"), vec![1, 0, 0, 0, 3, b'h', 195, 169]);
}

#[test]
fn ppm_reader_preserves_binary_whitespace_pixels_and_rotation_dimensions() {
    let mut stream = Cursor::new(
        b"P6\n# screen\n2 1\n255\n\x20\x0a\x0d\xff\x00\x80P6\n1 2\n255\n\x00\x00\x00\xff\xff\xff",
    );
    let frame = read_ppm(&mut stream).unwrap().unwrap();
    assert_eq!((frame.width, frame.height), (2, 1));
    assert_eq!(frame.rgb, [32, 10, 13, 255, 0, 128]);
    let rotated = read_ppm(&mut stream).unwrap().unwrap();
    assert_eq!((rotated.width, rotated.height), (1, 2));
    assert!(read_ppm(&mut stream).unwrap().is_none());
}

#[test]
fn malformed_or_truncated_frames_are_rejected_without_large_allocations() {
    for bytes in [
        b"P6\n0 1\n255\n".as_slice(),
        b"P6\n65535 65535\n255\n",
        b"P6\n1 1\n255\n\x00",
        b"P3\n1 1\n255\n",
    ] {
        assert!(read_ppm(&mut Cursor::new(bytes)).is_err());
    }
}

#[test]
fn rendering_averages_pixels_and_can_emit_plain_or_truecolor_ascii() {
    let frame = screen_ascii::frame::Frame {
        width: 2,
        height: 1,
        rgb: vec![0, 0, 0, 255, 255, 255],
    };
    let view = Viewport {
        x: 0,
        y: 0,
        cols: 2,
        rows: 1,
        width: 2,
        height: 1,
    };
    assert_eq!(
        render::detailed_lines(&frame, view, " .#", false, false, render::Mode::Ascii),
        vec![" #"]
    );
    assert_eq!(
        render::detailed_lines(&frame, view, " .#", false, true, render::Mode::Ascii),
        vec!["# "]
    );
    let one = Viewport { cols: 1, ..view };
    assert_eq!(
        render::detailed_lines(&frame, one, " .#", false, false, render::Mode::Ascii),
        vec!["."]
    );
    assert!(
        render::detailed_lines(&frame, view, " .#", true, false, render::Mode::Ascii)[0]
            .contains("\x1b[38;2;255;255;255m#")
    );
}

#[test]
fn scroll_packet_uses_signed_fixed_point_and_no_mouse_buttons() {
    assert_eq!(
        protocol::scroll((1, 2), (1080, 1920), -1),
        vec![
            3, 0, 0, 0, 1, 0, 0, 0, 2, 4, 56, 7, 128, 0, 0, 248, 0, 0, 0, 0, 0,
        ]
    );
}

#[test]
fn long_text_is_truncated_on_a_utf8_boundary() {
    let value = format!("{}é", "a".repeat(299));
    let packet = protocol::text(&value);
    assert_eq!(&packet[1..5], &[0, 0, 1, 43]);
    assert_eq!(packet.len(), 304);
    assert!(std::str::from_utf8(&packet[5..]).is_ok());
}

#[test]
fn block_rendering_keeps_top_and_bottom_colours_in_each_cell() {
    use screen_ascii::render::{Mode, detailed_lines};
    let frame = screen_ascii::frame::Frame {
        width: 1,
        height: 2,
        rgb: vec![255, 0, 0, 0, 0, 255],
    };
    let view = Viewport {
        x: 0,
        y: 0,
        cols: 1,
        rows: 1,
        width: 1,
        height: 2,
    };
    let output = detailed_lines(&frame, view, " .#", true, false, Mode::Blocks);
    assert!(output[0].contains("\x1b[38;2;255;0;0m"));
    assert!(output[0].contains("\x1b[48;2;0;0;255m▀"));
    assert_eq!(
        detailed_lines(&frame, view, " .#", false, false, Mode::Blocks)[0]
            .chars()
            .filter(|ch| *ch == '▀')
            .count(),
        1
    );
}

#[test]
fn ascii_ui_background_is_quiet_and_dark_detail_stays_visible() {
    use screen_ascii::render::{Mode, detailed_lines};
    let frame = screen_ascii::frame::Frame {
        width: 2,
        height: 1,
        rgb: vec![25, 25, 25, 70, 70, 70],
    };
    let view = Viewport {
        x: 0,
        y: 0,
        cols: 2,
        rows: 1,
        width: 2,
        height: 1,
    };
    let output = detailed_lines(&frame, view, " .:-=+*#%@", true, false, Mode::Ascii);
    assert!(output[0].starts_with("\x1b[38;2;110;110;110m "));
}
