use super::*;

#[test]
fn source_build_consumer_generation_fails_with_provenance() {
    let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", None);
    assert!(err.is_err_and(|err| {
        err.to_string()
            .contains("consumer_requires_release_install")
    }));
}

#[test]
fn consumer_manifest_mismatch_and_bad_target_fail() {
    let err = consumer_acquire_from("ubuntu-26.04", "9.9.9", Some(&test_manifest_json()));
    assert!(err.is_err_and(|err| err.to_string().contains("version_mismatch")));
    let err = consumer_acquire_from(
        "ubuntu-26.04-arm",
        env!("CARGO_PKG_VERSION"),
        Some(&test_manifest_json()),
    );
    assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), Some("not json"));
    assert!(err.is_err());
}

#[test]
fn consumer_gate_rejects_attacker_manifests() {
    let version = env!("CARGO_PKG_VERSION");
    let sha = "a".repeat(64);
    let manifest = |repository: &str, artifact: &str| {
        format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"{repository}\",\"commit\":\"{}\",\"targets\":[{{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"{artifact}\",\"sha256\":\"{sha}\"}}]}}",
            "a".repeat(40)
        )
    };
    let bound = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-x86_64-unknown-linux-gnu"
    );
    for (repository, artifact) in [
        ("evil/velnor-new", bound.as_str()),
        (
            "tailrocks/velnor-new",
            "https://evil.example/r/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        ),
        ("tailrocks/velnor-new", "https://github.com@evil.example/x"),
    ] {
        let json = manifest(repository, artifact);
        assert!(
            consumer_acquire_from("ubuntu-26.04", version, Some(&json)).is_err(),
            "accepted {repository} {artifact}"
        );
    }
}

#[test]
fn consumer_manifest_without_commit_fails() {
    let full = test_manifest_json();
    let segment = format!("\"commit\":\"{}\",", "a".repeat(40));
    assert!(full.contains(&segment), "fixture must carry commit");
    let missing = full.replace(&segment, "");
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), Some(&missing))
        .expect_err("commit required");
    assert!(err.to_string().contains("commit"), "{err}");
    let malformed = full.replace(&segment, "\"commit\":\"xyz\",");
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), Some(&malformed))
        .expect_err("malformed commit fails");
    assert!(err.to_string().contains("malformed_commit"), "{err}");
}

#[test]
fn fixture_manifest_embeds_runner_target_record() {
    let step = consumer_acquire_from(
        "ubuntu-26.04",
        env!("CARGO_PKG_VERSION"),
        Some(&test_manifest_json()),
    )
    .map(|step| step.name);
    assert_eq!(
        step.map_err(|err| err.to_string()),
        Ok("Acquire Velnor".to_owned())
    );
}

#[test]
fn consumer_acquire_uses_native_checksum_utility_for_each_target() {
    let manifest = test_manifest_json();
    for (label, target) in [
        ("ubuntu-26.04", ReleaseTarget::LinuxX86_64),
        ("macos-15", ReleaseTarget::MacosArm64),
        ("macos-15-intel", ReleaseTarget::MacosX86_64),
    ] {
        let step = consumer_acquire_from(label, env!("CARGO_PKG_VERSION"), Some(&manifest))
            .expect("consumer acquire for supported target");
        assert_native_checksum_utility(step, target);
    }
}

#[test]
fn lock_acquire_uses_native_checksum_utility_for_each_target() {
    use velnor_actions_contract_release::{GeneratorBinary, LockedGenerator, MiseBootstrap};

    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            commit: "a".repeat(40),
            binaries: ReleaseTarget::ALL
                .into_iter()
                .map(|target| GeneratorBinary {
                    target: target.triple().to_owned(),
                    artifact: format!(
                        "https://github.com/tailrocks/velnor-new/releases/download/v{}/velnor-actions-{}-{}",
                        env!("CARGO_PKG_VERSION"),
                        env!("CARGO_PKG_VERSION"),
                        target.triple()
                    ),
                    sha256: "b".repeat(64),
                })
                .collect(),
        },
        mise_bootstrap: MiseBootstrap {
            version: MISE_VERSION.to_owned(),
            artifact: "https://example.invalid/mise".to_owned(),
            sha256: "c".repeat(64),
        },
        actions: Vec::new(),
    };
    for (label, target) in [
        ("ubuntu-26.04", ReleaseTarget::LinuxX86_64),
        ("macos-15", ReleaseTarget::MacosArm64),
        ("macos-15-intel", ReleaseTarget::MacosX86_64),
    ] {
        let step = lock_acquire_step(&lock, label, "$RUNNER_TEMP/velnor/bin/velnor-actions-test")
            .expect("lock acquire for supported target");
        assert_native_checksum_utility(step, target);
    }
}

fn assert_native_checksum_utility(
    step: velnor_actions_contract_workflow::Step,
    target: ReleaseTarget,
) {
    let velnor_actions_contract_workflow::StepKind::Shell { run, .. } = step.kind else {
        panic!("Acquire must be a shell step");
    };
    let script = run.join(" ");
    let expected = match target {
        ReleaseTarget::LinuxX86_64 => "sha256sum",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "shasum -a 256",
    };
    let other = match target {
        ReleaseTarget::LinuxX86_64 => "shasum -a 256",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "sha256sum",
    };
    assert_eq!(
        script.matches(&format!("{expected} -c -")).count(),
        2,
        "{script}"
    );
    assert!(!script.contains(other), "{script}");
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
    use velnor_actions_contract_config::config::{CheckExecutor, CheckPlatform, CheckRunner};
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
        let velnor_actions_contract_workflow::StepKind::Shell { run, env } = step.kind else {
            panic!("acquisition must be shell");
        };
        assert!(run.iter().any(|arg| arg.contains("shasum -a 256 -c -")));
        assert!(env.values().any(|value| value.ends_with(platform.target())));
        assert!(!run.iter().any(|arg| arg.contains("sha256sum")));
    }
}

#[test]
fn acquisition_rejects_tampered_platform() {
    use velnor_actions_contract_config::config::{CheckExecutor, CheckPlatform, CheckRunner};
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
        if target != ReleaseTarget::LinuxX86_64 {
            assert!(!argv[2].contains("sha256sum"), "{target:?}: {}", argv[2]);
        }
    }
}

#[test]
fn setup_uses_extracted_binary_digest_for_each_platform() {
    use velnor_actions_workflow_steps::setup::{
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

    assert_eq!(linux.version, MISE_VERSION);
    assert_eq!(linux.sha256, MISE_BINARY_SHA256_LINUX_X64);
    assert_eq!(macos.version, MISE_VERSION);
    assert_eq!(macos.sha256, MISE_BINARY_SHA256_MACOS_ARM64);
    assert_ne!(linux.sha256, macos.sha256);
}
