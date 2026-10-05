use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, PullRequestCachePolicy, ResourcesConfig, StacksConfig,
    TestShardingConfig, VerificationRunner, WorkflowConfig, WorkflowPolicy,
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
            pull_request_cache_policy: PullRequestCachePolicy::default(),
            runner_label: None,
            tasks: Vec::new(),
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
        execution: None,
    }
}

#[test]
fn source_build_consumer_generation_fails_with_provenance() {
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), None);
    assert!(err.is_err_and(|err| {
        err.to_string()
            .contains("consumer_requires_release_install")
    }));
}

#[test]
fn consumer_manifest_mismatch_and_bad_target_fail() {
    let err = consumer_acquire_from("ubuntu-26.04", "9.9.9", Some(&test_manifest_json()));
    assert!(err.is_err_and(|err| err.to_string().contains("version_mismatch")));
    let version = env!("CARGO_PKG_VERSION");
    let err = consumer_acquire_from("ubuntu-26.04-arm", version, Some(&test_manifest_json()));
    assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
    let err = consumer_acquire_from("ubuntu-26.04", version, Some("not json"));
    assert!(err.is_err());
}

#[test]
fn consumer_gate_rejects_attacker_manifests() {
    let version = env!("CARGO_PKG_VERSION");
    let sha = "a".repeat(64);
    let manifest = |repository: &str, first_artifact: &str| {
        let targets = velnor_actions_contract::SUPPORTED_TARGETS
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let artifact = if index == 0 {
                    first_artifact.to_owned()
                } else {
                    format!(
                        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/{}",
                        velnor_actions_contract::asset_filename(version, target)
                    )
                };
                format!(
                    "{{\"target\":\"{target}\",\"artifact\":\"{artifact}\",\"sha256\":\"{sha}\"}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"{repository}\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
            "a".repeat(40),
        )
    };
    let bound = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-x86_64-unknown-linux-gnu"
    );
    for (repository, artifact, expected) in [
        ("evil/velnor-new", bound.as_str(), "unexpected_repository"),
        (
            "tailrocks/velnor-new",
            "https://evil.example/r/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
            "unexpected_artifact_url",
        ),
        (
            "tailrocks/velnor-new",
            "https://github.com@evil.example/x",
            "unexpected_artifact_url",
        ),
    ] {
        let json = manifest(repository, artifact);
        let err = consumer_acquire_from("ubuntu-26.04", version, Some(&json))
            .expect_err("attacker manifest must be rejected");
        assert!(
            err.to_string().contains(expected),
            "expected {expected} for {repository} {artifact}, got {err}"
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
fn setup_uses_extracted_binary_digest_for_each_platform() {
    use velnor_actions_workflow_renderer::setup::{
        MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
    };
    let config = config_with(BTreeMap::new());
    for (target, expected) in [
        (ReleaseTarget::MacosArm64, MISE_BINARY_SHA256_MACOS_ARM64),
        (ReleaseTarget::MacosX86_64, MISE_BINARY_SHA256_MACOS_X64),
    ] {
        let setup = resolve_mise_setup_for_release_target(&config, target).expect("verified setup");
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
