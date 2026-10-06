use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use screen_ascii::{
    geometry::Viewport,
    input::TouchState,
    session::{parse_version, select_device},
};
fn event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}
fn view() -> Viewport {
    Viewport {
        x: 10,
        y: 2,
        cols: 20,
        rows: 10,
        width: 200,
        height: 400,
    }
}
#[test]
fn dragging_keeps_a_single_finger_and_releases_even_outside_the_view() {
    let mut state = TouchState::default();
    let down = state.handle(
        event(MouseEventKind::Down(MouseButton::Left), 10, 2),
        view(),
    );
    assert_eq!(down.len(), 1);
    assert_eq!(down[0][1], 0);
    let movement = state.handle(
        event(MouseEventKind::Drag(MouseButton::Left), 20, 7),
        view(),
    );
    assert_eq!(movement[0][1], 2);
    assert_eq!(&movement[0][10..18], &[0, 0, 0, 105, 0, 0, 0, 220]);
    let up = state.handle(event(MouseEventKind::Up(MouseButton::Left), 0, 0), view());
    assert_eq!(up[0][1], 1);
    assert_eq!(&up[0][10..18], &[0, 0, 0, 105, 0, 0, 0, 220]);
    assert!(state.cancel().is_empty());
}
#[test]
fn margins_do_not_tap_and_cancel_releases_a_pressed_finger() {
    let mut state = TouchState::default();
    assert!(
        state
            .handle(event(MouseEventKind::Down(MouseButton::Left), 0, 0), view())
            .is_empty()
    );
    state.handle(
        event(MouseEventKind::Down(MouseButton::Left), 10, 2),
        view(),
    );
    assert_eq!(state.cancel()[0][1], 1);
    assert!(
        state
            .handle(
                event(MouseEventKind::Drag(MouseButton::Left), 10, 2),
                view()
            )
            .is_empty()
    );
}
#[test]
fn device_selection_rejects_unauthorized_offline_and_ambiguous_devices() {
    let output = "List of devices attached\nphone device\nother unauthorized\nghost offline\n";
    assert_eq!(select_device(output, None).unwrap(), "phone");
    assert!(select_device(output, Some("other")).is_err());
    assert!(select_device("List of devices attached\n", None).is_err());
    assert!(select_device("a device\nb device\n", None).is_err());
    assert_eq!(
        select_device("a device\nb device\n", Some("b")).unwrap(),
        "b"
    );
}
#[test]
fn server_version_is_parsed_and_unsupported_versions_are_rejected() {
    assert_eq!(
        parse_version("scrcpy 5.0 <https://github.com/Genymobile/scrcpy>\n\nDependencies:")
            .unwrap(),
        "5.0"
    );
    assert!(parse_version("scrcpy 99.0").is_err());
    assert!(parse_version("scrcpy 5.0;whoami").is_err());
}
