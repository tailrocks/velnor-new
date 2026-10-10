use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::{ExecutionConfig, RoutingWorkflow, VelnorConfig};

#[test]
fn qualification_uses_the_current_rust_catalog_and_reviewed_manifest() {
    let config = config_with(BTreeMap::new());
    let pins = resolve_rust_toolchain_qualification(&config).expect("reviewed Rust pins resolve");
    assert_eq!(pins.mise_setup.version, velnor_actions_mise::MISE_VERSION);
    assert_eq!(
        pins.mise_setup.sha256,
        velnor_actions_workflow_renderer::setup::MISE_BINARY_SHA256_LINUX_X64
    );
    assert_eq!(pins.mbx_version, velnor_actions_mise::MR_BOXINGTON_VERSION);
    assert_eq!(pins.rust_version, "1.99.0");
    assert_eq!(
        pins.manifest_url,
        "https://static.rust-lang.org/dist/channel-rust-1.99.0.toml"
    );
    assert_eq!(
        pins.manifest_sha256,
        velnor_actions_workflow_renderer::RUST_TOOLCHAIN_QUALIFICATION_MANIFEST_SHA256
    );
}

#[test]
fn schema2_qualification_routes_catalog_aligned_rust_pins() {
    let mut config = config_with(BTreeMap::new());
    config.schema = VelnorConfig::SCHEMA_V2;
    let mut execution =
        ExecutionConfig::hosted_default("ubuntu-26.04").expect("hosted execution fixture is valid");
    execution.workflows.insert(RoutingWorkflow::Qualification);
    config.execution = Some(execution);
    let files = crate::routing::extra_files(&config, env!("CARGO_PKG_VERSION"))
        .expect("qualification workflow renders");
    let qualification = files
        .iter()
        .find(|file| file.path == velnor_actions_workflow_renderer::schema2::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");
    let yaml = qualification.bytes.as_str();
    assert!(yaml.contains("rust-toolchain-linux-x64"));
    assert!(yaml.contains("rust-toolchain-macos-arm64"));
    assert!(yaml.contains("channel-rust-1.99.0.toml"));
}
