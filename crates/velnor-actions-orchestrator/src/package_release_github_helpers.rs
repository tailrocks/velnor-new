//! Pure byte and path helpers shared by authenticated GitHub API adapters.

use sha2::{Digest, Sha256};

pub(in crate::package_release_publish) fn is_lowercase_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(in crate::package_release_publish) fn sha256(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        push_hex_byte(&mut out, byte, false);
    }
    out
}

pub(in crate::package_release_publish) fn encode_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            push_hex_byte(&mut out, byte, true);
        }
    }
    out
}

fn push_hex_byte(output: &mut String, byte: u8, uppercase: bool) {
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    let hex = if uppercase { UPPER } else { LOWER };
    output.push(char::from(hex[usize::from(byte >> 4)]));
    output.push(char::from(hex[usize::from(byte & 0x0f)]));
}
