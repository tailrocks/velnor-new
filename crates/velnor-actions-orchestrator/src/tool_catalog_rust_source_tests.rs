//! `[stacks.rust] toolchain_source = "mise"` enforcement tests.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::config::RustToolchainSource;
use velnor_actions_contract::{RustStackConfig, VelnorConfig};
use velnor_actions_mise::PinnedTool;

use super::{MISSING_SOURCE_CODE, SECOND_SOURCE_CODE};
use crate::pins::tests::config_with;
use crate::tool_catalog_resolve::resolve_tool_catalog;

const MISE: &str = "[tools]\nrust = { version = \"1.99.1\", profile = \"minimal\", components = [\"clippy\", \"rustfmt\"] }\n";

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

#[test]
fn mise_only_source_resolves_the_mise_pin() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    let catalog = resolve_tool_catalog(dir.path(), &opted_in()).expect("resolve");
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
    assert!(catalog.exports_rustup_toolchain());
}

#[test]
fn agreeing_rust_toolchain_toml_next_to_the_mise_pin_is_named() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(
        dir.path(),
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.99.1\"\n",
    );
    let err = error_for(dir.path(), &opted_in());
    assert!(
        err.contains(&format!(
            "{SECOND_SOURCE_CODE}:rust-toolchain.toml=1.99.1,mise.toml=1.99.1"
        )),
        "{err}"
    );
    assert!(err.contains("delete rust-toolchain.toml"), "{err}");
}

#[test]
fn disagreeing_rust_toolchain_toml_reports_the_second_source_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(
        dir.path(),
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.98.1\"\n",
    );
    let err = error_for(dir.path(), &opted_in());
    assert!(
        err.contains(&format!(
            "{SECOND_SOURCE_CODE}:rust-toolchain.toml=1.98.1,mise.toml=1.99.1"
        )),
        "{err}"
    );
}

#[test]
fn legacy_toolchain_file_and_tool_versions_are_second_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), "rust-toolchain", "1.99.1\n");
    assert!(error_for(dir.path(), &opted_in()).contains("rust-toolchain=1.99.1"));

    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(dir.path(), ".tool-versions", "rust 1.99.1\n");
    assert!(error_for(dir.path(), &opted_in()).contains(".tool-versions=1.99.1"));
}

#[test]
fn nested_toolchain_file_is_a_second_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(
        dir.path(),
        "tools/logo/rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.99.1\"\n",
    );
    let err = error_for(dir.path(), &opted_in());
    assert!(
        err.contains("tools/logo/rust-toolchain.toml=1.99.1,mise.toml=1.99.1"),
        "{err}"
    );
}

#[test]
fn velnor_config_rust_pins_are_second_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    let mut cfg = opted_in();
    cfg.tools.overrides = BTreeMap::from([("rust".to_owned(), "1.99.1".to_owned())]);
    assert!(error_for(dir.path(), &cfg).contains("tools.rust=1.99.1,mise.toml=1.99.1"));

    let mut cfg = opted_in();
    if let Some(rust) = cfg.stacks.rust.as_mut() {
        rust.toolchain = Some("1.99.1".to_owned());
    }
    assert!(error_for(dir.path(), &cfg).contains("stacks.rust.toolchain=1.99.1"));
}

#[test]
fn opted_in_project_without_a_mise_rust_pin_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", "[tools]\nnode = \"24.0.0\"\n");
    assert!(error_for(dir.path(), &opted_in()).contains(MISSING_SOURCE_CODE));
}

#[test]
fn without_the_option_an_agreeing_toolchain_file_stays_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "mise.toml", MISE);
    write(
        dir.path(),
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.99.1\"\n",
    );
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
