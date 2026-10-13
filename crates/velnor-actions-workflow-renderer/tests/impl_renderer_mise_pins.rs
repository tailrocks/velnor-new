use super::impl_renderer_fixtures::mise;

#[test]
fn target_selects_exact_2026_10_7_binary_digest_and_rejects_stale_pin() {
    for (target, expected) in [
        (
            "x86_64-unknown-linux-gnu",
            "6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85",
        ),
        (
            "aarch64-apple-darwin",
            "f5171e341518a57e8c4e9280e28443e35d66212c51164c83be76794e0a78b014",
        ),
        (
            "x86_64-apple-darwin",
            "c3355f0c56d1b9fe73a2ba30e034b4e483541b25b1ad812a87440abfaeec8baa",
        ),
    ] {
        let resolved = mise().for_target(target).expect("official target resolves");
        assert_eq!(resolved.version, "2026.10.7", "{target}");
        assert_eq!(resolved.sha256, expected, "{target}");
    }

    let mut stale = mise();
    stale.version = "2026.10.6".to_owned();
    assert!(
        stale.for_target("x86_64-unknown-linux-gnu").is_err(),
        "previous Mise release must not remain qualified"
    );
}
