use crate::{geometry::Viewport, protocol};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

#[derive(Default)]
pub struct TouchState {
    active: Option<((u32, u32), (u16, u16))>,
}

impl TouchState {
    pub fn handle(&mut self, event: MouseEvent, view: Viewport) -> Vec<Vec<u8>> {
        let point = view.point(event.column, event.row);
        let size = (view.width, view.height);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(point) = point {
                    let mut packets = self.cancel();
                    self.active = Some((point, size));
                    packets.push(protocol::touch(0, point, size));
                    packets
                } else {
                    vec![]
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let (Some(point), Some(_)) = (point, self.active) {
                    self.active = Some((point, size));
                    vec![protocol::touch(2, point, size)]
                } else {
                    vec![]
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let (Some(point), Some(_)) = (point, self.active) {
                    self.active = Some((point, size));
                }
                self.cancel()
            }
            MouseEventKind::Down(MouseButton::Right) => {
                vec![protocol::key(0, 4), protocol::key(1, 4)]
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                vec![protocol::key(0, 3), protocol::key(1, 3)]
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if let Some(point) = point {
                    vec![protocol::scroll(
                        point,
                        size,
                        if event.kind == MouseEventKind::ScrollUp {
                            1
                        } else {
                            -1
                        },
                    )]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    pub fn cancel(&mut self) -> Vec<Vec<u8>> {
        self.active
            .take()
            .map(|(point, size)| vec![protocol::touch(1, point, size)])
            .unwrap_or_default()
    }
}
