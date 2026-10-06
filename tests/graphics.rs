use base64::{Engine, engine::general_purpose::STANDARD};
use screen_ascii::{frame::Frame, geometry::Viewport, graphics};
use std::io::Cursor;

#[test]
fn graphics_preserves_pixels_dimensions_and_mouse_placement() {
    let frame = Frame {
        width: 2,
        height: 1,
        rgb: vec![255, 0, 0, 0, 0, 255],
    };
    let view = Viewport {
        x: 3,
        y: 1,
        cols: 12,
        rows: 6,
        width: 2,
        height: 1,
    };
    let mut output = Vec::new();
    graphics::write_frame(&mut output, &frame, view, true, false).unwrap();
    let wire = String::from_utf8(output).unwrap();
    assert!(wire.starts_with("\x1b_Ga=T,f=100,t=d,i=31,p=1,q=2,C=1,c=12,r=6,m="));
    let encoded: String = wire
        .split("\x1b_G")
        .skip(1)
        .map(|chunk| {
            let (header, payload) = chunk.split_once(';').unwrap();
            assert!(header.contains("m="));
            let payload = payload.strip_suffix("\x1b\\").unwrap();
            assert!(payload.len() <= 4096);
            payload
        })
        .collect();
    assert!(wire.contains("m=0;"));
    let png = STANDARD.decode(encoded).unwrap();
    let mut decoder = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size()];
    let info = decoder.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (2, 1));
    assert_eq!(&pixels[..info.buffer_size()], &[255, 0, 0, 0, 0, 255]);
}

#[test]
fn large_images_use_complete_bounded_chunks() {
    let frame = Frame {
        width: 96,
        height: 96,
        rgb: {
            let mut state = 1234567_u32;
            (0..96 * 96 * 3)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect()
        },
    };
    let view = Viewport::fit(80, 24, 96, 96, 0.5);
    let mut output = Vec::new();
    graphics::write_frame(&mut output, &frame, view, false, true).unwrap();
    let wire = String::from_utf8(output).unwrap();
    let chunks: Vec<_> = wire.split("\x1b_G").skip(1).collect();
    assert!(chunks.len() > 1);
    for (index, chunk) in chunks.iter().enumerate() {
        let (header, payload) = chunk.split_once(';').unwrap();
        assert!(header.ends_with(if index + 1 == chunks.len() {
            "m=0"
        } else {
            "m=1"
        }));
        assert!(payload.strip_suffix("\x1b\\").unwrap().len() <= 4096);
    }
    let encoded: String = chunks
        .iter()
        .map(|c| c.split_once(';').unwrap().1.strip_suffix("\x1b\\").unwrap())
        .collect();
    let mut reader = png::Decoder::new(Cursor::new(STANDARD.decode(encoded).unwrap()))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert!(
        pixels[..info.buffer_size()]
            .chunks_exact(3)
            .all(|p| p[0] == p[1] && p[1] == p[2])
    );
}
