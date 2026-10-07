//! Helper acquisition from release provenance.

use velnor_actions_contract_release::{ReleaseTarget, SUPPORTED_TARGETS};
use velnor_actions_orchestrator_pins::pins::{
    acquire_script_argv, consumer_acquire_step_with_manifest,
};

/// Fixture manifest matching the workspace version.
fn manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "a".repeat(40)
    )
}

#[test]
fn missing_manifest_fails_with_provenance() {
    let err = consumer_acquire_step_with_manifest("ubuntu-26.04", "0.1.0", None)
        .expect_err("provenance required");
    assert!(
        err.to_string()
            .contains("consumer_requires_release_install"),
        "{err}"
    );
}

#[test]
fn version_mismatch_fails_closed() {
    let err = consumer_acquire_step_with_manifest("ubuntu-26.04", "9.9.9", Some(&manifest_json()))
        .expect_err("mismatch");
    assert!(err.to_string().contains("version_mismatch"), "{err}");
}

#[test]
fn malformed_manifest_fails_closed() {
    consumer_acquire_step_with_manifest(
        "ubuntu-26.04",
        env!("CARGO_PKG_VERSION"),
        Some("not json"),
    )
    .expect_err("malformed");
}

#[test]
fn bad_runner_label_fails_closed() {
    let err = consumer_acquire_step_with_manifest(
        "windows-latest",
        env!("CARGO_PKG_VERSION"),
        Some(&manifest_json()),
    )
    .expect_err("bad target");
    assert!(
        err.to_string().contains("unsupported_target_for_runner"),
        "{err}"
    );
}

#[test]
fn acquire_script_verifies_with_platform_digest_tool() {
    let linux = acquire_script_argv(
        "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.1",
        "/tmp/seed",
        ReleaseTarget::LinuxX86_64,
    )
    .expect("linux");
    assert!(
        linux.iter().any(|arg| arg.contains("sha256sum")),
        "{linux:?}"
    );
    let macos = acquire_script_argv(
        "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.1",
        "/tmp/seed",
        ReleaseTarget::MacosArm64,
    )
    .expect("macos");
    assert!(macos.iter().any(|arg| arg.contains("shasum")), "{macos:?}");
}

#[test]
fn acquire_script_rejects_bad_paths() {
    acquire_script_argv(
        "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.1",
        "relative/seed",
        ReleaseTarget::LinuxX86_64,
    )
    .expect_err("relative seed");
    acquire_script_argv("nope", "/tmp/seed", ReleaseTarget::LinuxX86_64).expect_err("bad staged");
}
