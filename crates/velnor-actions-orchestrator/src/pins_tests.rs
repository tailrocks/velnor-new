use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, PullRequestCachePolicy, ResourcesConfig, StacksConfig,
    TestShardingConfig, WorkflowConfig, WorkflowPolicy,
};

/// Config carrying exactly the given action-pin overrides.
pub(super) fn config_with(overrides: BTreeMap<String, ActionPinOverride>) -> VelnorConfig {
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
            tofu_apply: None,
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
    use velnor_actions_contract::{GeneratorBinary, LockedGenerator, MiseBootstrap};

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

fn assert_native_checksum_utility(step: velnor_actions_contract::Step, target: ReleaseTarget) {
    let velnor_actions_contract::StepKind::Shell { run, .. } = step.kind else {
        panic!("Acquire must be a shell step");
    };
    let script = run.join(" ");
    let expected = match target {
        ReleaseTarget::LinuxX86_64 => "sha256sum",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "shasum -a 256",
    };
    let other = match target {
        ReleaseTarget::LinuxX86_64 => "shasum",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "sha256sum",
    };
    assert_eq!(
        script.matches(&format!("{expected} -c -")).count(),
        2,
        "{script}"
    );
    assert!(
        script.contains(&format!("echo \"$p$s\"|{expected} -c -")),
        "{script}"
    );
    assert!(script.contains("cp \"$s\" \"$d\""), "{script}");
    assert!(
        script.contains("curl -fsSL --retry 5 --retry-all-errors"),
        "{script}"
    );
    assert!(
        script.contains(&format!("echo \"$p$d\"|{expected} -c -")),
        "{script}"
    );
    assert!(!script.contains(other), "{script}");
}
