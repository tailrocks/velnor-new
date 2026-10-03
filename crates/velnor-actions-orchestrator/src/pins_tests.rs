use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, ResourcesConfig, StacksConfig, TestShardingConfig,
    WorkflowConfig, WorkflowPolicy,
};

/// Config carrying exactly the given action-pin overrides.
fn config_with(overrides: BTreeMap<String, ActionPinOverride>) -> VelnorConfig {
    VelnorConfig {
        checks: Vec::new(),
        qualified_tools: Vec::new(),
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
        },
        resources: ResourcesConfig {
            compiler_process_budget: 2,
            test_process_budget: 2,
        },
        test_sharding: TestShardingConfig {
            default_shards: 1,
            by_manifest: BTreeMap::new(),
        },
        stacks: StacksConfig {
            ignore: Vec::new(),
            rust: None,
            tofu: None,
        },
        discovery: DiscoveryConfig {
            exclude: Vec::new(),
        },
        actions: ActionsConfig { overrides },
    }
}

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
    let err = consumer_acquire_from("ubuntu-26.04-arm", "0.1.0", Some(&test_manifest_json()));
    assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
    let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", Some("not json"));
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
fn acquisition_rejects_tampered_platform_and_unsupported_target() {
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
    assert!(
        acquire_argv(
            "${{ runner.temp }}/velnor/bin/velnor-actions-0.1.0",
            "aarch64-unknown-linux-gnu"
        )
        .is_err()
    );
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
