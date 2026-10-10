//! Pins follow-up cases: generator tags, Mise setup, acquisition.
//!
//! Declared via `#[path]` from `pins.rs` under `cfg(test)`; split from
//! `pins_tests.rs` by the 400-line repo-size gate.

use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::config::ActionPinOverride;
use velnor_actions_contract::{BuildTaskRunner, VerificationRunner};

#[test]
fn consumer_manifest_generator_tag_matches_source_commit() {
    let version = env!("CARGO_PKG_VERSION");
    let commit = "a".repeat(40);
    let version_tag = format!("/download/v{version}/");
    let generator_tag = format!("/download/generator-{commit}/");
    let valid = test_manifest_json().replace(&version_tag, &generator_tag);
    assert!(
        valid.contains(&generator_tag),
        "fixture must use generator tag"
    );
    assert!(consumer_acquire_from("ubuntu-26.04", version, Some(&valid)).is_ok());

    let wrong_commit = "b".repeat(40);
    let wrong_tag = format!("/download/generator-{wrong_commit}/");
    let mismatched = valid.replace(&generator_tag, &wrong_tag);
    assert!(
        consumer_acquire_from("ubuntu-26.04", version, Some(&mismatched)).is_err(),
        "generator tag must bind to manifest source commit"
    );
}

#[test]
fn mise_setup_defaults_to_compiled_pins() {
    let setup = resolve_mise_setup(&config_with(BTreeMap::new()), "ubuntu-26.04")
        .map_err(|err| err.to_string());
    assert_eq!(
        setup,
        Ok(MiseSetup {
            uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
            version: MISE_VERSION.to_owned(),
            sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        })
    );
}

#[test]
fn consumer_binary_mise_setup_uses_latest_verified_platforms_only() {
    let config = config_with(BTreeMap::new());
    for (target, digest) in [
        (
            ReleaseTarget::LinuxX86_64,
            "6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85",
        ),
        (
            ReleaseTarget::MacosArm64,
            "f5171e341518a57e8c4e9280e28443e35d66212c51164c83be76794e0a78b014",
        ),
    ] {
        let setup = resolve_mise_setup_for_consumer_binary_release(&config, target)
            .expect("supported consumer runner");
        assert_eq!(setup.version, "2026.10.7");
        assert_eq!(setup.sha256, digest);
    }
    assert!(
        resolve_mise_setup_for_consumer_binary_release(&config, ReleaseTarget::MacosX86_64)
            .is_err()
    );
}

#[test]
fn mise_setup_accepts_approved_override_only() {
    let approved = BTreeMap::from([(
        MISE_ACTION_KEY.to_owned(),
        ActionPinOverride {
            sha: MISE_ACTION_SHA.to_owned(),
            version: MISE_ACTION_VERSION.to_owned(),
        },
    )]);
    let setup = resolve_mise_setup(&config_with(approved), "ubuntu-26.04");
    assert!(setup.is_ok_and(|setup| setup.uses.ends_with(MISE_ACTION_SHA)));
    for pin in [
        ActionPinOverride {
            sha: "0".repeat(40),
            version: MISE_ACTION_VERSION.to_owned(),
        },
        ActionPinOverride {
            sha: MISE_ACTION_SHA.to_owned(),
            version: "v9.9.9".to_owned(),
        },
        ActionPinOverride {
            sha: "short".to_owned(),
            version: MISE_ACTION_VERSION.to_owned(),
        },
    ] {
        let overrides = BTreeMap::from([(MISE_ACTION_KEY.to_owned(), pin)]);
        assert!(resolve_mise_setup(&config_with(overrides), "ubuntu-26.04").is_err());
    }
}

#[test]
fn mise_setup_rejects_non_linux_runners() {
    for label in ["ubuntu-26.04-arm", "windows-latest", "ubuntu-latest"] {
        let err = resolve_mise_setup(&config_with(BTreeMap::new()), label);
        assert!(
            err.is_err_and(|err| err.to_string().contains("mise_setup_unsupported_target")),
            "{label}"
        );
    }
}

#[test]
fn macos_helper_asset_and_native_digest_match_runner() {
    use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner};
    for (label, platform) in [
        ("macos-15", CheckPlatform::MacosArm64),
        ("macos-15-intel", CheckPlatform::MacosX64),
    ] {
        let runner = CheckRunner {
            label: label.to_owned(),
            platform,
            executor: CheckExecutor::Hosted,
            container: None,
        };
        let step = consumer_acquire_for_runner(
            &runner,
            env!("CARGO_PKG_VERSION"),
            Some(&test_manifest_json()),
        )
        .expect("qualified helper");
        let velnor_actions_contract::StepKind::Shell { run, env } = step.kind else {
            panic!("acquisition must be shell");
        };
        assert!(run.iter().any(|arg| arg.contains("shasum -a 256 -c -")));
        assert!(env.values().any(|value| value.ends_with(platform.target())));
        assert!(!run.iter().any(|arg| arg.contains("sha256sum")));
    }
}

#[test]
fn acquisition_rejects_tampered_platform() {
    use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner};
    let runner = CheckRunner {
        label: "macos-15".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    };
    assert!(
        consumer_acquire_for_runner(
            &runner,
            env!("CARGO_PKG_VERSION"),
            Some(&test_manifest_json())
        )
        .is_err()
    );
}

#[test]
fn acquisition_template_selects_native_checksum_by_typed_target() {
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    for (target, expected) in [
        (ReleaseTarget::LinuxX86_64, "sha256sum -c -"),
        (ReleaseTarget::MacosArm64, "shasum -a 256 -c -"),
        (ReleaseTarget::MacosX86_64, "shasum -a 256 -c -"),
    ] {
        let argv = acquire_script_argv(&staged, "/opt/velnor/seed", target)
            .expect("supported typed target");
        assert!(argv[2].contains(expected), "{target:?}: {}", argv[2]);
        assert!(
            argv[2].contains("--proto '=https' --tlsv1.2"),
            "{target:?}: {}",
            argv[2]
        );
        assert!(
            argv[2].contains("s=\"/opt/velnor/seed/generator/${d##*/}\""),
            "{target:?}: {}",
            argv[2]
        );
        assert!(argv[2].contains(&staged), "{target:?}: {}", argv[2]);
        assert!(
            argv[2].contains("cp \"$s\" \"$d\""),
            "{target:?}: {}",
            argv[2]
        );
        if target != ReleaseTarget::LinuxX86_64 {
            assert!(!argv[2].contains("sha256sum"), "{target:?}: {}", argv[2]);
        }
    }
}

#[test]
fn setup_uses_extracted_binary_digest_for_each_platform() {
    use velnor_actions_workflow_renderer::setup::{
        MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
    };
    let config = config_with(BTreeMap::new());
    for (label, expected) in [
        ("macos-15", MISE_BINARY_SHA256_MACOS_ARM64),
        ("macos-15-intel", MISE_BINARY_SHA256_MACOS_X64),
    ] {
        let setup = resolve_mise_setup(&config, label).expect("verified setup");
        assert_eq!(setup.sha256, expected);
        assert_ne!(setup.sha256, MISE_BINARY_SHA256_LINUX_X64);
    }
}

#[test]
fn verification_mise_setup_pins_each_runner_architecture() {
    let config = config_with(BTreeMap::new());
    let linux = resolve_verification_mise_setup(&config, VerificationRunner::LinuxX64)
        .expect("Linux Mise pin");
    let macos = resolve_verification_mise_setup(&config, VerificationRunner::MacosArm64)
        .expect("Apple ARM64 Mise pin");
    let macos_26 = resolve_verification_mise_setup(&config, VerificationRunner::Macos26Arm64)
        .expect("macOS 26 Apple ARM64 Mise pin");

    assert_eq!(linux.version, MISE_VERSION);
    assert_eq!(linux.sha256, MISE_BINARY_SHA256_LINUX_X64);
    assert_eq!(macos.version, MISE_VERSION);
    assert_eq!(macos.sha256, MISE_BINARY_SHA256_MACOS_ARM64);
    assert_eq!(macos_26.version, MISE_VERSION);
    assert_eq!(macos_26.sha256, MISE_BINARY_SHA256_MACOS_ARM64);
    assert_ne!(linux.sha256, macos.sha256);
}

#[test]
fn native_build_task_mise_setup_uses_macos_arm64_binary_pin() {
    let setup =
        resolve_build_task_mise_setup(&config_with(BTreeMap::new()), BuildTaskRunner::Macos26Arm64)
            .expect("macOS 26 Mise pin");

    assert_eq!(setup.version, MISE_VERSION);
    assert_eq!(setup.sha256, MISE_BINARY_SHA256_MACOS_ARM64);
}
