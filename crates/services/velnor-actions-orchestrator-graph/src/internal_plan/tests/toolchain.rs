//! Toolchain and runner platform identity regressions.

use super::*;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};

#[test]
fn compiler_spec_versions_flip_the_digest() {
    let inputs = |tools: Vec<String>| ToolchainInputs {
        tools,
        components: vec!["clippy".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let pinned = inputs(vec!["rust@1.98.1".to_owned()]);
    let bumped = inputs(vec!["rust@1.99.0".to_owned()]);
    assert_ne!(
        toolchain_id(&pinned).expect("digest"),
        toolchain_id(&bumped).expect("digest")
    );
    // The toolchain builder feeds the same sorted specs into the same
    // digest, so a catalog pin change propagates to every task digest.
    let catalog = ToolCatalog::pinned();
    let group = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        CompileDriver::Cargo,
        TestRunner::CargoTest,
    );
    let built = toolchain_inputs_for(&group, &catalog).expect("rust toolchain inputs");
    assert_eq!(
        toolchain_id(&built).expect("digest"),
        toolchain_digest_for(&group, &catalog).expect("digest")
    );
    assert!(built.tools.iter().any(|spec| spec.starts_with("rust@")));
    assert_eq!(
        built.components,
        vec!["clippy".to_owned(), "rustfmt".to_owned()]
    );
    let imaged = platform_id_for_group("ubuntu-26.04", &group).expect("platform");
    let older = platform_id_for_group("ubuntu-24.04", &group).expect("platform");
    assert_ne!(imaged, older);
    assert!(validate_digest(&imaged).is_ok());
    let mut alien = group.clone();
    alien.identity.target = "riscv64-unknown-linux-gnu".to_owned();
    let err = platform_id_for_group("ubuntu-26.04", &alien).expect_err("target");
    assert!(err.to_string().contains("unsupported_target"), "{err}");
    for label in ["ubuntu-26.04-arm", "macos-15", "windows-2025", ""] {
        let err = platform_id_for_group(label, &group).expect_err("label");
        assert!(
            err.to_string().contains("unsupported_target_for_runner"),
            "{err}"
        );
    }
}
