//! Generator identity resolution and verification (P03).
//!
//! Declared via `#[path]` from `cover_identity.rs` (no `lib.rs` edit).
//! A generator SHA is verifiable only when it names the running binary:
//! its real SHA-256 hash. All-zero, empty, and unresolved-marker SHAs
//! prove nothing and never validate evidence; no lock fill ever upgrades
//! them, so a source build can never emit a release-pinned identity.

use crate::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA;

/// Lookup-skipped reason for an unverifiable source-build generator.
pub(crate) const SOURCE_BUILD_REASON: &str = "generator_unverifiable_source_build";

/// True for generator SHAs that prove nothing: empty, all-zero, or the
/// explicit unresolved marker. No release binary stands behind any of
/// them, so baseline evidence bound to them is unverifiable.
pub(crate) fn is_source_build(sha: &str) -> bool {
    sha.is_empty()
        || sha == UNRESOLVED_GENERATOR_SHA
        || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

/// SHA-256 hex of the running executable, comparable to release pins.
///
/// Root cause of the b3-vs-SHA256 incomparability: the native `b3-`
/// digest can never equal a 64-hex release pin. Recording the real
/// SHA-256 makes executable-against-release comparison structural;
/// provenance matches this value against the manifest pin exactly.
pub(crate) fn current_exe_sha256() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    Some(sha256_hex(&bytes))
}

/// SHA-256 hex over bytes via the pinned `sha2` crate.
///
/// Generator identity anchors on audited primitives, never hand-rolled
/// crypto: a subtle padding or schedule bug here would false-accept an
/// attacker binary as a release pin or false-reject a real one.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
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
#[cfg(test)]
mod tests;
