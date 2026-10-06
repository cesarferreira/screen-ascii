use screen_ascii::video::decode_stream;
use std::{
    io::Cursor,
    process::Command,
    sync::{Arc, Mutex},
};

#[test]
#[ignore = "requires FFmpeg with libx264 on PATH"]
fn decoder_preserves_new_dimensions_after_android_rotation() {
    let mut video = b"h264".to_vec();
    for size in ["160x320", "320x160"] {
        let generated = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=red:s={size}:r=10"),
                "-frames:v",
                "5",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-f",
                "h264",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(generated.status.success());
        let (width, height): (u32, u32) = if size == "160x320" {
            (160, 320)
        } else {
            (320, 160)
        };
        video.extend_from_slice(&[128, 0, 0, 0]);
        video.extend_from_slice(&width.to_be_bytes());
        video.extend_from_slice(&height.to_be_bytes());
        video.extend_from_slice(&[0; 8]);
        video.extend_from_slice(&(generated.stdout.len() as u32).to_be_bytes());
        video.extend_from_slice(&generated.stdout);
    }
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&sizes);
    decode_stream(
        &mut Cursor::new(video),
        "ffmpeg",
        Arc::new(move |frame| observed.lock().unwrap().push((frame.width, frame.height))),
    )
    .unwrap();
    let sizes = sizes.lock().unwrap().clone();
    assert_eq!(
        sizes,
        vec![
            (160, 320),
            (160, 320),
            (160, 320),
            (160, 320),
            (160, 320),
            (320, 160),
            (320, 160),
            (320, 160),
            (320, 160),
            (320, 160)
        ]
    );
}

#[test]
#[ignore = "requires FFmpeg with libx264 on PATH"]
fn empty_capture_session_can_reset_before_any_video_frame() {
    let generated = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x320:r=10",
            "-frames:v",
            "1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-tune",
            "zerolatency",
            "-f",
            "h264",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(generated.status.success());
    let mut video = b"h264".to_vec();
    // An entirely empty session, followed by a codec-configuration-only session.
    for _ in 0..2 {
        video.extend_from_slice(&[128, 0, 0, 0, 0, 0, 0, 160, 0, 0, 1, 64]);
    }
    let idr = generated
        .stdout
        .windows(4)
        .position(|window| window[..3] == [0, 0, 1] && window[3] & 31 == 5)
        .unwrap();
    video.extend_from_slice(&[64, 0, 0, 0, 0, 0, 0, 0]);
    video.extend_from_slice(&(idr as u32).to_be_bytes());
    video.extend_from_slice(&generated.stdout[..idr]);
    video.extend_from_slice(&[128, 0, 0, 0, 0, 0, 0, 160, 0, 0, 1, 64]);
    video.extend_from_slice(&[0; 8]);
    video.extend_from_slice(&(generated.stdout.len() as u32).to_be_bytes());
    video.extend_from_slice(&generated.stdout);
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&sizes);
    decode_stream(
        &mut Cursor::new(video),
        "ffmpeg",
        Arc::new(move |frame| observed.lock().unwrap().push((frame.width, frame.height))),
    )
    .unwrap();
    assert_eq!(*sizes.lock().unwrap(), vec![(160, 320)]);
}
