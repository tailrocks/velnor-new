//! Plan-identity envelope tests (P03 matrix unit cases).
//!
//! Declared via `#[path]` from `internal_plan.rs` under `cfg(test)`.

use super::*;

use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};

/// Digest over every envelope dimension for flip comparisons.
fn digest_full(
    package_id: &str,
    kind: TaskKind,
    configuration: &str,
    toolchain: &str,
    platform: &str,
    argv: &[String],
    closure: &str,
) -> String {
    let group = TaskGroup {
        task_id: format!("stack/rust/root/{}/default", kind.as_str()),
        package_id: package_id.to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: configuration.to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    let generator = default_generator();
    let extension = StackExtension {
        schema: "rust-task-identity-v1".to_owned(),
        data: serde_json::json!({}),
    };
    task_identity_digest(&IdentityInputs {
        task: &task,
        argv,
        toolchain_id: toolchain,
        platform_id: platform,
        manifest: "Cargo.toml",
        generator: &generator,
        extension,
        closure_digest: closure,
    })
    .expect("digest")
}

/// Digest for `package_id`/`kind` with a fixed envelope otherwise.
fn digest_for(package_id: &str, kind: TaskKind) -> String {
    digest_full(
        package_id,
        kind,
        "default",
        &digest_b3(b"toolchain"),
        &digest_b3(b"platform"),
        &["clippy".to_owned()],
        &digest_b3(b"closure"),
    )
}

#[test]
fn envelope_normalizes_components_and_binds_env() {
    assert_eq!(
        digest_for("path+file:///old/checkout#demo@0.1.0", TaskKind::Clippy),
        digest_for("demo@0.1.0", TaskKind::Clippy)
    );
    assert_ne!(
        digest_for("demo@0.1.0", TaskKind::Clippy),
        digest_for("other@0.1.0", TaskKind::Clippy)
    );
    assert_ne!(
        digest_for("demo@0.1.0", TaskKind::Clippy),
        digest_for("demo@0.1.0", TaskKind::Doc)
    );
    let generator = default_generator();
    assert!(!generator.sha256.bytes().all(|b| b == b'0'));
    assert!(!generator.sha256.is_empty());
}

#[test]
fn envelope_binds_platform_toolchain_config_argv() {
    let base = digest_for("demo@0.1.0", TaskKind::Clippy);
    let flip = |configuration: &str, toolchain: &str, platform: &str, argv: &[String]| {
        digest_full(
            "demo@0.1.0",
            TaskKind::Clippy,
            configuration,
            toolchain,
            platform,
            argv,
            &digest_b3(b"closure"),
        )
    };
    let toolchain = digest_b3(b"toolchain");
    let platform = digest_b3(b"platform");
    let argv = vec!["clippy".to_owned()];
    // Runner-image (platform), compiler (toolchain), profile
    // (configuration), and argv changes each invalidate the digest.
    assert_ne!(
        base,
        flip("default", &toolchain, &digest_b3(b"platform-22-04"), &argv)
    );
    assert_ne!(
        base,
        flip("default", &digest_b3(b"toolchain-1-99"), &platform, &argv)
    );
    assert_ne!(base, flip("ci", &toolchain, &platform, &argv));
    assert_ne!(
        base,
        flip(
            "default",
            &toolchain,
            &platform,
            &["clippy".to_owned(), "--fix".to_owned()]
        )
    );
}

#[test]
fn envelope_binds_input_closure_digest() {
    let base = digest_for("demo@0.1.0", TaskKind::Clippy);
    let edited = digest_full(
        "demo@0.1.0",
        TaskKind::Clippy,
        "default",
        &digest_b3(b"toolchain"),
        &digest_b3(b"platform"),
        &["clippy".to_owned()],
        &digest_b3(b"closure-edited"),
    );
    assert_ne!(
        base, edited,
        "a source edit flips the closure digest and must flip input_digest"
    );
}

/// Tofu proposal via the T12 adapter constructor.
fn tofu_proposal(kind: velnor_actions_tofu_core::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu_core::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn tofu_metadata_and_cache_ids_derive() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let meta = adapter_metadata(&task, &[]).expect("metadata");
    assert_eq!(meta["unit_id"], serde_json::json!("root"));
    assert_eq!(meta["compile_driver"], serde_json::json!("tofu"));
    assert_eq!(meta["test_runner"], serde_json::json!("none"));
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let toolchain = toolchain_id(&task, &catalog).expect("toolchain");
    let ids = cache_ids_for(&task, "ubuntu-24.04", &toolchain).expect("cache ids");
    assert_eq!(
        ids.cache_format_id(),
        identities::cache_format_id_for_tofu().as_str()
    );
}
