//! Tool-catalog resolution tests.
//!
//! Declared via `#[path]` from `tool_catalog_resolve.rs` under `cfg(test)`.

use std::collections::BTreeMap;

use velnor_actions_contract::{RustStackConfig, VelnorConfig};
use velnor_actions_mise::PinnedTool;

use super::resolve_tool_catalog;
use crate::pins::tests::config_with;

#[test]
fn resolves_repo_toolchain_file_and_omits_rustup_toolchain_export() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.94.1\"\n",
    )
    .expect("write toolchain");
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.94.1");
    assert!(!catalog.exports_rustup_toolchain());
    assert_eq!(catalog.rustup_toolchain_env(), None);
}

#[test]
fn rejects_conflict_between_repo_toolchain_file_and_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.94.1\"\n",
    )
    .expect("write toolchain");
    let mut cfg = config_with(BTreeMap::new());
    let mut rust = RustStackConfig::default_config();
    rust.toolchain = Some("1.98.1".to_owned());
    cfg.stacks.rust = Some(rust);
    let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("conflict must fail");
    assert!(
        err.to_string().contains(
            "conflicting_rust_toolchain:rust-toolchain.toml=1.94.1,stacks.rust.toolchain=1.98.1"
        ),
        "got {err}"
    );
}

#[test]
fn resolves_config_only_rust_and_tool_overrides_with_rustup_export() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg: VelnorConfig = config_with(BTreeMap::new());
    cfg.tools.overrides = BTreeMap::from([
        ("rust".to_owned(), "1.94.1".to_owned()),
        ("cargo-nextest".to_owned(), "0.9.150".to_owned()),
        ("actionlint".to_owned(), "1.7.13".to_owned()),
    ]);
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.94.1");
    assert_eq!(catalog.version(PinnedTool::Nextest), "0.9.150");
    assert_eq!(catalog.version(PinnedTool::Actionlint), "1.7.13");
    assert!(catalog.exports_rustup_toolchain());
    assert_eq!(catalog.rustup_toolchain_env(), Some("1.94.1"));
}

#[test]
fn resolves_target_qualified_exact_toolchain_and_rejects_non_exact_channels() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config_with(BTreeMap::new());
    std::fs::write(
        dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.99.1-x86_64-unknown-linux-gnu\"\n",
    )
    .expect("write target-qualified toolchain");
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve target-qualified");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
    assert!(!catalog.exports_rustup_toolchain());

    std::fs::write(
        dir.path().join("rust-toolchain"),
        "1.99.1-aarch64-apple-darwin\n",
    )
    .expect("write legacy toolchain with different target");
    let conflict_err =
        resolve_tool_catalog(dir.path(), &cfg).expect_err("target mismatch must fail");
    assert!(
        conflict_err.to_string().contains(
            "conflicting_rust_toolchain_files:rust-toolchain.toml=1.99.1-x86_64-unknown-linux-gnu,rust-toolchain=1.99.1-aarch64-apple-darwin"
        ),
        "got {conflict_err}"
    );
    std::fs::remove_file(dir.path().join("rust-toolchain")).expect("remove legacy toolchain");

    for bad in [
        "[toolchain]\nchannel = \"stable\"\n",
        "[toolchain]\nchannel = \"1.99\"\n",
        "[toolchain]\nchannel = \"nightly-2026-10-01\"\n",
        "[toolchain]\n",
        "[[[\n",
    ] {
        std::fs::write(dir.path().join("rust-toolchain.toml"), bad).expect("write bad");
        let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("non-exact must fail");
        assert!(
            err.to_string().contains("invalid_rust_toolchain:"),
            "expected invalid_rust_toolchain for {bad:?}, got {err}"
        );
    }
}

#[test]
fn rust_mr_boxington_option_carries_the_repo_pin_into_the_catalog() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { version = \"1.94.1\", mr_boxington = true }\nmr-boxington = \"1.24.0\"\n",
    )
    .expect("write mise.toml");
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(catalog.rust_mr_boxington());
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.24.0");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.98.1");
}

#[test]
fn rust_mr_boxington_option_accepts_a_table_formed_mr_boxington_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { mr_boxington = true }\n\n[tools.mr-boxington]\nversion = \"1.24.0\"\n",
    )
    .expect("write mise.toml");
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(catalog.rust_mr_boxington());
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.24.0");
}

#[test]
fn rust_mr_boxington_option_without_a_record_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { version = \"1.94.1\", mr_boxington = true }\n",
    )
    .expect("write mise.toml");
    let cfg = config_with(BTreeMap::new());
    let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("missing record must fail");
    assert!(
        err.to_string()
            .contains("rust_mr_boxington_requires_mr_boxington_record"),
        "got {err}"
    );
}

#[test]
fn rust_mr_boxington_option_with_an_inexact_pin_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { version = \"1.94.1\", mr_boxington = true }\nmr-boxington = \"1.24\"\n",
    )
    .expect("write mise.toml");
    let cfg = config_with(BTreeMap::new());
    let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("inexact pin must fail");
    assert!(
        err.to_string()
            .contains("rust_mr_boxington_requires_exact_pin:1.24"),
        "got {err}"
    );
}

#[test]
fn rust_mr_boxington_option_conflicting_with_the_config_override_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { version = \"1.94.1\", mr_boxington = true }\nmr-boxington = \"1.24.0\"\n",
    )
    .expect("write mise.toml");
    let mut cfg = config_with(BTreeMap::new());
    cfg.tools
        .overrides
        .insert("mr-boxington".to_owned(), "1.21.1".to_owned());
    let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("conflict must fail");
    assert!(
        err.to_string()
            .contains("ambiguous_mr_boxington_pin:mise.toml=1.24.0,.velnor/config.toml=1.21.1"),
        "got {err}"
    );
}

#[test]
fn rust_mr_boxington_option_matching_the_config_override_resolves_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { version = \"1.94.1\", mr_boxington = true }\nmr-boxington = \"1.24.0\"\n",
    )
    .expect("write mise.toml");
    let mut cfg = config_with(BTreeMap::new());
    cfg.tools
        .overrides
        .insert("mr-boxington".to_owned(), "1.24.0".to_owned());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(catalog.rust_mr_boxington());
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.24.0");
}

#[test]
fn rust_mr_boxington_option_absent_leaves_the_catalog_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = \"1.98.1\"\nmr-boxington = \"1.21.1\"\n",
    )
    .expect("write mise.toml");
    let mut cfg = config_with(BTreeMap::new());
    cfg.tools
        .overrides
        .insert("mr-boxington".to_owned(), "1.20.0".to_owned());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(!catalog.rust_mr_boxington());
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.20.0");
}

#[test]
fn dot_mise_toml_supplies_the_wrapper_option_when_mise_toml_is_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".mise.toml"),
        "[tools]\nrust = { version = \"1.94.1\", mr_boxington = true }\nmr-boxington = \"1.24.0\"\n",
    )
    .expect("write .mise.toml");
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(catalog.rust_mr_boxington());
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.24.0");
}

#[test]
fn malformed_mise_toml_stays_a_finding_and_resolution_continues() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.toml"), "[tools\nrust = \n").expect("write bad");
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(
        !catalog.rust_mr_boxington(),
        "malformed bytes contribute no option; the tooling-input finding reports them"
    );
    assert_eq!(
        catalog.version(PinnedTool::MrBoxington),
        velnor_actions_mise::catalog::MR_BOXINGTON_VERSION
    );
}

#[test]
fn malformed_mise_toml_does_not_hide_the_dot_spelling_option() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.toml"), "[tools\nrust = \n").expect("write bad");
    std::fs::write(
        dir.path().join(".mise.toml"),
        "[tools]\nrust = { mr_boxington = true }\nmr-boxington = \"1.24.0\"\n",
    )
    .expect("write .mise.toml");
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
    assert!(catalog.rust_mr_boxington());
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.24.0");
}

#[test]
fn malformed_mise_toml_selecting_the_wrapper_fails_closed_on_the_missing_pin() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Malformed TOML whose line-local bytes still select the wrapper —
    // exactly what the P06 driver detector reads — must not degrade to a
    // silent flag-off while detection picks the MBX driver.
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools\nrust = \nmr_boxington = true # stray option line\n",
    )
    .expect("write bad");
    let cfg = config_with(BTreeMap::new());
    let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("wrapper without pin must fail");
    assert!(
        err.to_string()
            .contains("rust_mr_boxington_requires_mr_boxington_record"),
        "got {err}"
    );
}

#[test]
fn os_restricted_mr_boxington_records_fail_closed_naming_the_filter() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nrust = { mr_boxington = true }\nmr-boxington = { version = \"1.24.0\", os = [\"macos\", \"linux\"] }\n",
    )
    .expect("write mise.toml");
    let cfg = config_with(BTreeMap::new());
    let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("os filter must fail");
    assert!(
        err.to_string()
            .contains("rust_mr_boxington_requires_unfiltered_pin:os=macos,linux"),
        "got {err}"
    );
}
