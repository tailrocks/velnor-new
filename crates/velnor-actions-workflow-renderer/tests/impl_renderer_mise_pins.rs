use super::impl_renderer_fixtures::mise;

#[test]
fn target_selects_exact_2026_10_9_binary_digest_and_rejects_stale_pin() {
    for (target, expected) in [
        (
            "x86_64-unknown-linux-gnu",
            "6ab6646b52b83f6bbd51c9d0d3ae54264ffa90efd1ac6d7c0ec5a86bbc6a0030",
        ),
        (
            "aarch64-apple-darwin",
            "93a0703c25fc15cd4fbe02019c9ea23ca9045e077af5e96c01207e1346edb14a",
        ),
        (
            "x86_64-apple-darwin",
            "8e243d9afcb73f3ee3ea35998e61f828cc0fc4a1cb0962defa702e54454bc1a7",
        ),
    ] {
        let resolved = mise().for_target(target).expect("official target resolves");
        assert_eq!(resolved.version, "2026.10.9", "{target}");
        assert_eq!(resolved.sha256, expected, "{target}");
    }

    let mut stale = mise();
    stale.version = "2026.10.6".to_owned();
    assert!(
        stale.for_target("x86_64-unknown-linux-gnu").is_err(),
        "previous Mise release must not remain qualified"
    );
}
