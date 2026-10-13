//! Plan-identity envelope tests (P03 matrix unit cases).
//!
//! Declared via `#[path]` from `internal_plan.rs` under `cfg(test)`.

use super::*;
use velnor_actions_rust::TaskKind;

/// Digest over every envelope dimension for flip comparisons.
fn digest_full(
    package_id: &str,
    kind: TaskKind,
    configuration: &str,
    toolchain: &str,
    platform: &str,
    argv: &[String],
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
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    };
    let generator = default_generator();
    let extension = StackExtension {
        schema: "rust-task-identity-v1".to_owned(),
        data: serde_json::json!({}),
    };
    task_identity_digest(&IdentityInputs {
        group: &group,
        argv,
        toolchain_id: toolchain,
        platform_id: platform,
        manifest: "Cargo.toml",
        generator: &generator,
        extension,
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
