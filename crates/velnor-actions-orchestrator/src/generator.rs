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
mod tests {
    use super::*;
    use velnor_actions_contract::digest_b3;

    #[test]
    fn unverifiable_shas_detected() {
        assert!(is_source_build(""));
        assert!(is_source_build(&"0".repeat(64)));
        assert!(is_source_build(UNRESOLVED_GENERATOR_SHA));
        assert!(!is_source_build(&"1".repeat(64)));
        assert!(!is_source_build(&digest_b3(b"exe")));
    }

    #[test]
    fn sha256_matches_vectors_and_exe_verifies() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let exe = current_exe_sha256().expect("exe readable");
        assert_eq!(exe.len(), 64);
        assert!(exe.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!exe.starts_with("b3-"), "comparable to release pins");
    }

    /// Padding edges (55/56/64 bytes) plus multi-block NIST cases.
    ///
    /// Edge vectors come from an independent `hashlib` oracle; the
    /// two-block and million-byte cases are the published NIST values.
    #[test]
    fn sha256_matches_padding_edges_and_multiblock() {
        let range = |n: usize| {
            (0..n)
                .map(|b| u8::try_from(b).unwrap_or(0))
                .collect::<Vec<_>>()
        };
        for (len, hex) in [
            (
                55,
                "463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59",
            ),
            (
                56,
                "da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562",
            ),
            (
                64,
                "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108",
            ),
        ] {
            assert_eq!(sha256_hex(&range(len)), hex, "{len} bytes");
        }
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        );
    }
}
