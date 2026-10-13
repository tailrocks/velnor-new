use super::{MAX_STDOUT_BYTES, ObservationError};

/// Strict standard padded base64, including canonical unused trailing bits.
pub(super) fn decode(input: &str) -> Result<Vec<u8>, ObservationError> {
    if input.len() % 4 != 0 || input.len() > MAX_STDOUT_BYTES.div_ceil(3) * 4 {
        return Err(ObservationError::Bounds);
    }
    let mut result = Vec::with_capacity((input.len() / 4 * 3).min(MAX_STDOUT_BYTES));
    let chunks = input.as_bytes().chunks_exact(4);
    let count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        let a = digit(chunk[0])?;
        let b = digit(chunk[1])?;
        let last = index + 1 == count;
        result.push((a << 2) | (b >> 4));
        if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || b & 15 != 0 {
                return Err(ObservationError::InvalidEnvelope);
            }
        } else {
            let c = digit(chunk[2])?;
            result.push((b << 4) | (c >> 2));
            if chunk[3] == b'=' {
                if !last || c & 3 != 0 {
                    return Err(ObservationError::InvalidEnvelope);
                }
            } else {
                result.push((c << 6) | digit(chunk[3])?);
            }
        }
        if result.len() > MAX_STDOUT_BYTES {
            return Err(ObservationError::Bounds);
        }
    }
    Ok(result)
}

fn digit(byte: u8) -> Result<u8, ObservationError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(ObservationError::InvalidEnvelope),
    }
}
