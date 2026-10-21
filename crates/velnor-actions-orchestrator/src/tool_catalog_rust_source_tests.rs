//! `[stacks.rust] toolchain_source = "mise"` enforcement tests.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::config::RustToolchainSource;
use velnor_actions_contract::{RustStackConfig, VelnorConfig};
use velnor_actions_mise::PinnedTool;

use super::{DISAGREES_CODE, MISSING_SOURCE_CODE};
use crate::pins::tests::config_with;
use crate::tool_catalog_resolve::resolve_tool_catalog;

const MISE: &str =
    "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"1.26.0\"\n";

fn opted_in() -> VelnorConfig {
    let mut cfg = config_with(BTreeMap::new());
    let mut rust = RustStackConfig::default_config();
    rust.toolchain_source = Some(RustToolchainSource::Mise);
    cfg.stacks.rust = Some(rust);
    cfg
}

fn write(root: &Path, rel: &str, text: &str) {
    if let Some(parent) = Path::new(rel).parent() {
        std::fs::create_dir_all(root.join(parent)).expect("mkdir");
    }
    std::fs::write(root.join(rel), text).expect("write");
}

fn error_for(root: &Path, cfg: &VelnorConfig) -> String {
    resolve_tool_catalog(root, cfg)
        .expect_err("the Mise-only source must fail")
        .to_string()
}

/// The owner's standard `rust-toolchain.toml` for the Mise pin above.
const TOOLCHAIN: &str = "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n";

#[test]
fn mise_pin_alone_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    let catalog = resolve_tool_catalog(dir.path(), &opted_in()).expect("resolve");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
}

#[test]
fn the_owner_layout_keeps_an_agreeing_rust_toolchain_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), "rust-toolchain.toml", TOOLCHAIN);
    write(dir.path(), "tools/logo/rust-toolchain.toml", TOOLCHAIN);
    let catalog = resolve_tool_catalog(dir.path(), &opted_in()).expect("agreeing files resolve");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
}

#[test]
fn a_disagreeing_rust_toolchain_toml_names_both_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(
        dir.path(),
        "rust-toolchain.toml",
        &TOOLCHAIN.replace("1.99.1", "1.98.1"),
    );
    let err = error_for(dir.path(), &opted_in());
    assert!(
        err.contains(&format!(
            "{DISAGREES_CODE}:rust-toolchain.toml=1.98.1,mise.toml=1.99.1"
        )),
        "{err}"
    );
    assert!(
        err.contains("set rust-toolchain.toml to the same version"),
        "{err}"
    );
}

#[test]
fn legacy_toolchain_file_and_tool_versions_must_agree() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), "rust-toolchain", "1.98.1\n");
    assert!(error_for(dir.path(), &opted_in()).contains("rust-toolchain=1.98.1,mise.toml=1.99.1"));

    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), ".tool-versions", "rust 1.98.1\n");
    assert!(error_for(dir.path(), &opted_in()).contains(".tool-versions=1.98.1"));
}

#[test]
fn a_disagreeing_nested_toolchain_file_names_its_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), "rust-toolchain.toml", TOOLCHAIN);
    write(
        dir.path(),
        "tools/logo/rust-toolchain.toml",
        &TOOLCHAIN.replace("1.99.1", "1.98.1"),
    );
    let err = error_for(dir.path(), &opted_in());
    assert!(
        err.contains("tools/logo/rust-toolchain.toml=1.98.1,mise.toml=1.99.1"),
        "{err}"
    );
}

#[test]
fn velnor_config_rust_pins_must_agree() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    let mut cfg = opted_in();
    cfg.tools.overrides = BTreeMap::from([("rust".to_owned(), "1.99.1".to_owned())]);
    resolve_tool_catalog(dir.path(), &cfg).expect("an agreeing pin resolves");
    cfg.tools.overrides = BTreeMap::from([("rust".to_owned(), "1.98.1".to_owned())]);
    assert!(error_for(dir.path(), &cfg).contains("tools.rust=1.98.1,mise.toml=1.99.1"));
}

#[test]
fn opted_in_project_without_a_mise_rust_pin_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", "[tools]\nnode = \"24.0.0\"\n");
    write(dir.path(), "rust-toolchain.toml", TOOLCHAIN);
    assert!(error_for(dir.path(), &opted_in()).contains(MISSING_SOURCE_CODE));
}

#[test]
fn without_the_option_an_agreeing_toolchain_file_stays_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), "rust-toolchain.toml", TOOLCHAIN);
    let cfg = config_with(BTreeMap::new());
    let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("hard decision 4 default");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
}

#[test]
fn rust_toolchain_source_loads_only_the_mise_value() {
    let load = |stack: &str| {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            ".velnor/config.toml",
            &format!("schema = 1\n[stacks.rust]\n{stack}"),
        );
        crate::config::load_config(dir.path())
            .map(|config| config.stacks.rust.and_then(|rust| rust.toolchain_source))
    };
    assert_eq!(
        load("").expect("absent"),
        None,
        "absent keeps hard decision 4"
    );
    assert_eq!(
        load("toolchain_source = \"mise\"\n").expect("mise"),
        Some(RustToolchainSource::Mise)
    );
    for value in ["\"rust-toolchain\"", "\"\"", "true"] {
        assert!(
            load(&format!("toolchain_source = {value}\n")).is_err(),
            "{value} is rejected"
        );
    }
}
