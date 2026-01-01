use super::*;
use velnor_actions_contract::digest_b3;
use velnor_actions_orchestrator_core::sha256::sha256_hex;
use velnor_actions_orchestrator_graph::internal_plan::current_exe_sha256;

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
