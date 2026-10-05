// scrcpy's control protocol uses big-endian fields; -2 identifies a finger.
pub fn touch(action: u8, point: (u32, u32), size: (u16, u16)) -> Vec<u8> {
    let mut packet = vec![2, action];
    packet.extend_from_slice(&(-2_i64).to_be_bytes());
    position(&mut packet, point, size);
    packet.extend_from_slice(&(if action == 1 { 0_u16 } else { u16::MAX }).to_be_bytes());
    packet.extend_from_slice(&[0; 8]);
    packet
}

pub fn key(action: u8, code: u32) -> Vec<u8> {
    let mut packet = vec![0, action];
    packet.extend_from_slice(&code.to_be_bytes());
    packet.extend_from_slice(&[0; 8]);
    packet
}

pub fn text(value: &str) -> Vec<u8> {
    let mut end = value.len().min(300);
    while !value.is_char_boundary(end) {
        end -= 1
    }
    let mut packet = vec![1];
    packet.extend_from_slice(&(end as u32).to_be_bytes());
    packet.extend_from_slice(&value.as_bytes()[..end]);
    packet
}

pub fn scroll(point: (u32, u32), size: (u16, u16), vertical: i16) -> Vec<u8> {
    let mut packet = vec![3];
    position(&mut packet, point, size);
    packet.extend_from_slice(&0_i16.to_be_bytes());
    packet.extend_from_slice(
        &(vertical.clamp(-16, 16) as i32 * 2048)
            .clamp(-32768, 32767)
            .to_be_bytes()[2..],
    );
    packet.extend_from_slice(&[0; 4]);
    packet
}

fn position(packet: &mut Vec<u8>, point: (u32, u32), size: (u16, u16)) {
    packet.extend_from_slice(&point.0.to_be_bytes());
    packet.extend_from_slice(&point.1.to_be_bytes());
    packet.extend_from_slice(&size.0.to_be_bytes());
    packet.extend_from_slice(&size.1.to_be_bytes());
}
