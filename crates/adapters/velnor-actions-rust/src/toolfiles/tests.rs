use super::*;

#[test]
fn broadening_pins_root_lock_and_config_only() {
    assert_eq!(
        selection_broadening("Cargo.lock"),
        Some(SelectionBroadening::Lockfile)
    );
    for path in ["Cargo.toml", ".cargo/config.toml", ".cargo/config"] {
        assert_eq!(
            selection_broadening(path),
            Some(SelectionBroadening::RootConfig),
            "{path:?}"
        );
    }
    for path in [
        "crates/a/Cargo.lock",
        "crates/a/Cargo.toml",
        "src/lib.rs",
        ".mise.toml",
        "mise.toml",
        "rust-toolchain.toml",
    ] {
        assert_eq!(selection_broadening(path), None, "{path:?}");
    }
}

#[test]
fn known_toolfiles_cover_own_and_foreign() {
    for path in ["rust-toolchain.toml", "mise.toml", "mise.lock"] {
        assert!(is_known_toolfile(path), "{path:?}");
    }
    for path in [".mise.toml", "Cargo.toml", "Cargo.lock", "src/lib.rs"] {
        assert!(!is_known_toolfile(path), "{path:?}");
    }
}
