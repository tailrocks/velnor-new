//! Typed per-target Mise setup set resolution tests.
//!
//! Declared via `#[path]` from `pins.rs` under `cfg(test)` so the
//! pin-resolution tests keep the file size gate.

use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn mise_setup_set_reuses_configured_target_and_resolves_each_digest() {
    let config = config_with(BTreeMap::new());
    let linux = resolve_mise_setup(&config, "ubuntu-26.04").expect("Linux setup");
    let set = resolve_mise_setup_set(&config, "ubuntu-26.04", &linux).expect("each target pin");
    assert_eq!(
        set.for_target("x86_64-unknown-linux-gnu")
            .expect("Linux pin"),
        &linux
    );
    assert_eq!(
        set.for_target("aarch64-apple-darwin")
            .expect("macOS ARM pin")
            .sha256,
        MISE_BINARY_SHA256_MACOS_ARM64
    );
    assert_eq!(
        set.for_target("x86_64-apple-darwin")
            .expect("macOS Intel pin")
            .sha256,
        MISE_BINARY_SHA256_MACOS_X64
    );
    assert!(
        resolve_mise_setup_set(&config, "windows-latest", &linux)
            .is_err_and(|err| err.to_string().contains("mise_setup_unsupported_target"))
    );
}
