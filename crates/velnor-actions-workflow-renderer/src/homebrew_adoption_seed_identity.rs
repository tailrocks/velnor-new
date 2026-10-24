//! Seed release identity scalar validation and digest encoding.

use sha2::{Digest, Sha256};

pub(super) fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn version_at_least(actual: &str, minimum: &str) -> bool {
    fn parts(version: &str) -> Option<[u64; 3]> {
        let mut values = version.split('.');
        let result = [
            values.next()?.parse().ok()?,
            values.next()?.parse().ok()?,
            values.next()?.parse().ok()?,
        ];
        values.next().is_none().then_some(result)
    }
    parts(actual)
        .zip(parts(minimum))
        .is_some_and(|(actual, minimum)| actual >= minimum)
}
