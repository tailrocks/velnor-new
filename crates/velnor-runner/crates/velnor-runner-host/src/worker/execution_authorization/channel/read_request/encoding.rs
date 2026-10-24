use crate::error::HostError;

pub(super) const REQUEST_LIMIT: usize = 16 * 1024;

pub(super) fn decode_lower_hex(value: &str) -> Result<Vec<u8>, HostError> {
    if !value.len().is_multiple_of(2) || value.len() > REQUEST_LIMIT * 2 {
        return Err(HostError::Frame);
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(HostError::Frame);
    }
    for pair in pairs {
        let high = hex_nibble(pair[0]).ok_or(HostError::Frame)?;
        let low = hex_nibble(pair[1]).ok_or(HostError::Frame)?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
