use std::process::Command;
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ascii-scrcpy"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn demo_snapshot_renders_without_external_tools_or_a_tty() {
    let result = run(&["--demo", "--snapshot", "--render", "ascii", "--no-color"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains('@'));
    assert!(output.lines().count() >= 10);
    assert!(!output.contains('\x1b'));
}
#[test]
fn invalid_render_and_capture_options_fail_before_connecting() {
    for args in [
        vec!["--demo", "--char-aspect", "0"],
        vec!["--demo", "--max-fps", "0"],
        vec!["--demo", "--ramp", ""],
        vec!["--demo", "--ramp", "🦀"],
        vec!["--demo", "--max-size", "0"],
    ] {
        let result = run(&args);
        assert!(!result.status.success());
    }
}
