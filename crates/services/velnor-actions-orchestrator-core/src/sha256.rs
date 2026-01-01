//! SHA-256 hex helpers over the pinned `sha2` crate.

/// SHA-256 hex over bytes via the pinned `sha2` crate.
///
/// Generator identity anchors on audited primitives, never hand-rolled
/// crypto: a subtle padding or schedule bug here would false-accept an
/// attacker binary as a release pin or false-reject a real one.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex_lower(sha2::Sha256::digest(bytes).as_slice())
}

/// Lowercase hex encoding of digest bytes.
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
