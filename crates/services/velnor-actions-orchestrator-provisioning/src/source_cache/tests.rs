use super::*;

#[test]
fn cache_keys_reject_unsupported_execution_targets() {
    // Rust execution targets are charset-validated, so supported
    // configs can carry non-release triples; the cache path
    // hard-fails on them instead of keying a foreign toolchain.
    for target in [
        "aarch64-unknown-linux-gnu",
        "wasm32-unknown-unknown",
        "host",
    ] {
        let err = sources_cache_key(target, "1.98.1", &[]).expect_err("target");
        assert!(err.to_string().contains("bad_target"), "{target}: {err}");
    }
    for target in velnor_actions_contract_release::ReleaseTarget::ALL {
        let key = sources_cache_key(target.triple(), "1.98.1", &[]).expect("supported");
        assert!(key.starts_with(SOURCES_KEY_PREFIX), "{key}");
    }
    assert!(sources_cache_key("x86_64-unknown-linux-gnu", "1.98", &[]).is_err());
}
