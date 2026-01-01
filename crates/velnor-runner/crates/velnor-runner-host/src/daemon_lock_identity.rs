//! Docker-engine and journal-instance identity validation.

use sha2::{Digest, Sha256};

pub(crate) fn engine_key(engine_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(engine_id.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        hex.push(HEX[usize::from(byte >> 4)] as char);
        hex.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    hex
}

pub(crate) fn engine_id_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
}

pub(crate) fn instance_id_valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| b"0123456789abcdef".contains(&byte))
}
