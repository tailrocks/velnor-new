//! Pin resolution and fail-closed publication proofs.

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorBinary, GeneratorValidation, LockedGenerator, MiseBootstrap,
    ResourcesConfig, StacksConfig, StepKind, TestShardingConfig, WorkflowConfig, WorkflowPolicy,
};
use velnor_actions_workflow_renderer::steps::{ASSET_SHA_ENV, ASSET_URL_ENV, RELEASE_COMMIT_ENV};

/// Config carrying exactly the given action-pin overrides.
fn config_with(overrides: BTreeMap<String, ActionPinOverride>) -> VelnorConfig {
    VelnorConfig {
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
            verification: None,
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
            workloads: Vec::new(),
            ignore: Vec::new(),
            rust: None,
            tofu: None,
        },
        discovery: DiscoveryConfig {
            exclude: Vec::new(),
        },
        actions: ActionsConfig { overrides },
        delivery: Default::default(),
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

/// Lock fixture carrying one Linux generator asset and its source commit.
fn lock_with_provenance(artifact: &str, sha256: &str, commit: &str) -> GeneratorLock {
    GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            commit: commit.to_owned(),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: artifact.to_owned(),
                sha256: sha256.to_owned(),
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2026.9.18".to_owned(),
            artifact: "https://example.invalid/mise".to_owned(),
            sha256: "b".repeat(64),
        },
        actions: Vec::new(),
    }
}

/// The canonical acquire factory binds all three release provenance values.
fn assert_asset_provenance(step: &Step, artifact: &str, sha256: &str, commit: &str) {
    let StepKind::Shell { env, .. } = &step.kind else {
        panic!("Acquire step must be a shell step");
    };
    assert_eq!(env.get(ASSET_URL_ENV).map(String::as_str), Some(artifact));
    assert_eq!(env.get(ASSET_SHA_ENV).map(String::as_str), Some(sha256));
    assert_eq!(
        env.get(RELEASE_COMMIT_ENV).map(String::as_str),
        Some(commit)
    );
}

#[test]
fn manifest_and_lock_acquire_use_canonical_provenance_factory() {
    let version = env!("CARGO_PKG_VERSION");
    let manifest_json = test_manifest_json();
    let manifest = ReleaseManifest::parse_json(&manifest_json, "fixture-manifest.json")
        .expect("fixture manifest parses");
    let target = target_for_runner_label("ubuntu-26.04").expect("Linux runner target");
    let manifest_record = manifest
        .record_for_target(target)
        .expect("fixture manifest Linux record");
    let staged = format!("{STAGED_BINARY_PREFIX}{version}");
    let manifest_provenance = HelperProvenance::ReleaseAsset {
        url: manifest_record.artifact.clone(),
        sha256: manifest_record.sha256.clone(),
        commit: manifest.commit.clone(),
    };
    let consumer = consumer_acquire_from("ubuntu-26.04", version, Some(&manifest_json))
        .expect("manifest acquire");
    let expected_consumer =
        provision_acquire_step(&manifest_provenance, &staged).expect("canonical manifest acquire");
    assert_eq!(consumer, expected_consumer);
    assert_asset_provenance(
        &consumer,
        &manifest_record.artifact,
        &manifest_record.sha256,
        &manifest.commit,
    );

    let lock_artifact = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-lock-{version}-{target}"
    );
    let lock_sha256 = "b".repeat(64);
    let lock_commit = "c".repeat(40);
    let lock = lock_with_provenance(&lock_artifact, &lock_sha256, &lock_commit);
    let lock_provenance = HelperProvenance::ReleaseAsset {
        url: lock_artifact.clone(),
        sha256: lock_sha256.clone(),
        commit: lock_commit.clone(),
    };
    let locked = lock_acquire_step(&lock, "ubuntu-26.04", &staged).expect("lock acquire");
    let expected_lock =
        provision_acquire_step(&lock_provenance, &staged).expect("canonical lock acquire");
    assert_eq!(locked, expected_lock);
    assert_asset_provenance(&locked, &lock_artifact, &lock_sha256, &lock_commit);
}

#[test]
fn lock_acquire_rejects_malicious_staged_paths() {
    let lock = lock_with_provenance(
        "https://example.invalid/velnor-actions",
        &"a".repeat(64),
        &"d".repeat(40),
    );
    for staged in [
        format!("{STAGED_BINARY_PREFIX}../escape"),
        format!("{STAGED_BINARY_PREFIX}0.1.0/../../escape"),
        format!("{STAGED_BINARY_PREFIX}0.1.0$(touch-pwned)"),
    ] {
        let error = lock_acquire_step(&lock, "ubuntu-26.04", &staged)
            .expect_err("malicious staged path must fail closed");
        assert!(
            error.to_string().contains("early_acquire_staged_path"),
            "unexpected staged-path error for {staged}: {error}"
        );
    }
}

#[test]
fn mise_setup_rejects_absent_owned_qualification_before_emission() {
    let error = resolve_mise_setup(&config_with(BTreeMap::new()), "ubuntu-26.04")
        .expect_err("unpublished owned Mise must block workflow emission");
    assert!(error.to_string().contains("qualified distribution absent"));
    assert!(error.to_string().contains("Mise"));
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
    let config = config_with(approved);
    assert!(mise_action_uses(&config).is_ok_and(|uses| uses.ends_with(MISE_ACTION_SHA)));
    assert!(
        resolve_mise_setup(&config, "ubuntu-26.04")
            .is_err_and(|error| error.to_string().contains("qualified distribution absent"))
    );
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
