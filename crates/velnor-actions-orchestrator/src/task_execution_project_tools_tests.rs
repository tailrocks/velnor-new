use std::collections::BTreeMap;

use super::*;
use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::workflow::{TaskBindingDigest, TaskExecutionManifestEntryV1};

fn record() -> TaskExecutionManifestEntryV1 {
    TaskExecutionManifestEntryV1 {
        task_id: "stack/rust/demo/clippy/default".to_owned(),
        execution_digest: "b3-".to_owned() + &"a".repeat(64),
        task_digest: TaskBindingDigest::new("b3-".to_owned() + &"b".repeat(64))
            .expect("binding digest"),
        toolchain_inputs: ToolchainInputs {
            tools: vec!["rust@${VELNOR_TOOL_RUST}".to_owned()],
            components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
        },
        argv: vec!["rust@${VELNOR_TOOL_RUST}".to_owned()],
        env: BTreeMap::from([(
            "RUSTUP_TOOLCHAIN".to_owned(),
            "${VELNOR_TOOL_RUST}".to_owned(),
        )]),
        matrix_id: "rust|task:stack/rust/demo/clippy/default".to_owned(),
        matrix_key: "b3-".to_owned() + &"c".repeat(64),
        report_helper_version: "0.1.25".to_owned(),
        matrix_max_parallel: None,
    }
}

#[test]
fn project_version_references_resolve_to_exact_rust_version() {
    let resolved = resolve_with(&record(), |name| {
        (name == "VELNOR_TOOL_RUST").then(|| "1.95.2".to_owned())
    })
    .expect("version resolves");

    assert_eq!(resolved.argv, ["rust@1.95.2"]);
    assert_eq!(resolved.toolchain_inputs.tools, ["rust@1.95.2"]);
    assert_eq!(resolved.env["RUSTUP_TOOLCHAIN"], "1.95.2");
}

#[test]
fn invalid_or_missing_project_version_fails_closed() {
    assert!(resolve_with(&record(), |_| None).is_err());
    assert!(resolve_with(&record(), |_| Some("latest".to_owned())).is_err());
}
