//! Contract config, manifest, and action override cases.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ActionPin, ContractError, GeneratorBinary, GeneratorLock, GeneratorValidation, LockedGenerator,
    MiseBootstrap, ReleaseManifest, SUPPORTED_TARGETS, TargetRecord, WorkflowPolicy,
    asset_filename,
};

#[test]
fn config_validation_reports_key_paths() {
    use velnor_actions_contract::config::ActionsConfig;
    use velnor_actions_contract::config::RustReleaseConfig;
    use velnor_actions_contract::{
        DiscoveryConfig, PullRequestCachePolicy, ResourcesConfig, RustConfiguration,
        RustStackConfig, StacksConfig, TestShardingConfig, VelnorConfig, WorkflowConfig,
    };
    let valid = VelnorConfig {
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
            by_manifest: BTreeMap::from([("crates/large/Cargo.toml".to_owned(), 2)]),
        },
        stacks: StacksConfig {
            ignore: vec![],
            rust: Some(RustStackConfig {
                configurations: vec![RustConfiguration {
                    name: "default".to_owned(),
                    features: vec!["default".to_owned()],
                    target: "host".to_owned(),
                }],
                compile_driver: None,
                test_runner: None,
                run_ignored: None,
                release: RustReleaseConfig::default(),
            }),
            tofu: None,
        },
        discovery: DiscoveryConfig {
            exclude: vec!["vendor/**".to_owned()],
        },
        actions: ActionsConfig::default(),
        execution: None,
    };
    assert_eq!(valid.validate(".velnor/config.toml"), Ok(()));
    let support = WorkflowPolicy::ConsumerV1.support_workflow(GeneratorValidation::Bootstrap);
    assert!(support.validators.is_empty() && !support.candidate_validation);
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    assert_eq!(
        support.validators,
        velnor_actions_contract::ValidatorKind::repository_validators().to_vec()
    );
    assert!(support.candidate_validation);
    let mut bad = valid.clone();
    bad.schema = 2;
    assert!(bad.validate("cfg").is_err());
    let mut bad = valid.clone();
    bad.stacks.ignore = vec!["rust".to_owned(), "bogus".to_owned()];
    assert!(bad.validate("cfg").is_err());
    let mut bad = valid.clone();
    bad.discovery.exclude = vec!["/absolute/**".to_owned()];
    assert!(bad.validate("cfg").is_err());
    let mut bad = valid;
    bad.workflow.runner_label = Some("ubuntu-latest".to_owned());
    assert!(bad.validate("cfg").is_err());
}

#[test]
fn runner_label_uses_exact_catalog_match() {
    use velnor_actions_contract::config::ActionsConfig;
    use velnor_actions_contract::config::{LATEST_RUNNER_LABEL, RUNNER_LABEL_CATALOG};
    use velnor_actions_contract::{
        ContractError, DiscoveryConfig, PullRequestCachePolicy, ResourcesConfig, StacksConfig,
        TestShardingConfig, VelnorConfig,
    };
    let base = || VelnorConfig {
        checks: Vec::new(),
        qualified_tools: Vec::new(),
        schema: 1,
        workflow: velnor_actions_contract::WorkflowConfig {
            name: "CI".to_owned(),
            policy: velnor_actions_contract::WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: velnor_actions_contract::GeneratorValidation::Bootstrap,
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
            ignore: vec![],
            rust: None,
            tofu: None,
        },
        discovery: DiscoveryConfig { exclude: vec![] },
        actions: ActionsConfig::default(),
        execution: None,
    };
    assert!(RUNNER_LABEL_CATALOG.contains(&LATEST_RUNNER_LABEL));
    for label in RUNNER_LABEL_CATALOG {
        let mut config = base();
        config.workflow.runner_label = Some(label.to_owned());
        assert_eq!(config.validate("cfg"), Ok(()), "label {label}");
    }
    for label in [
        "ubuntu-latest",
        "ubuntu-24.04-latest",
        "ubuntu-99.04",
        "windows-latest",
        "",
        " ubuntu-24.04",
    ] {
        let mut config = base();
        config.workflow.runner_label = Some(label.to_owned());
        let Err(ContractError::Config {
            key_path, problem, ..
        }) = config.validate("cfg")
        else {
            panic!("label {label:?} must be rejected");
        };
        assert_eq!(key_path, "workflow.runner_label");
        assert!(problem.starts_with("unsupported_label:"), "got {problem}");
    }
}

#[test]
fn uppercase_rust_config_name_rejected_with_key_path() {
    use velnor_actions_contract::config::ActionsConfig;
    use velnor_actions_contract::config::RustReleaseConfig;
    use velnor_actions_contract::{
        ContractError, DiscoveryConfig, GeneratorValidation, PullRequestCachePolicy,
        ResourcesConfig, RustConfiguration, RustStackConfig, StacksConfig, TestShardingConfig,
        VelnorConfig, WorkflowConfig, WorkflowPolicy,
    };
    let mut config = VelnorConfig {
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
            ignore: vec![],
            rust: Some(RustStackConfig {
                configurations: vec![RustConfiguration {
                    name: "Default".to_owned(),
                    features: vec![],
                    target: "host".to_owned(),
                }],
                compile_driver: None,
                test_runner: None,
                run_ignored: None,
                release: RustReleaseConfig::default(),
            }),
            tofu: None,
        },
        discovery: DiscoveryConfig { exclude: vec![] },
        actions: ActionsConfig::default(),
        execution: None,
    };
    let Err(ContractError::Config {
        key_path, problem, ..
    }) = config.validate("cfg")
    else {
        panic!("uppercase name must be rejected");
    };
    assert_eq!(key_path, "stacks.rust.configurations.name");
    assert!(problem.starts_with("bad_component:"), "got {problem}");
    if let Some(rust) = config.stacks.rust.as_mut() {
        rust.configurations[0].name = "default".to_owned();
    }
    assert_eq!(config.validate("cfg"), Ok(()));
}

#[test]
fn actions_overrides_validate_allowlist_and_pin_shape() {
    use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
    let sha = "3d3c42e5aac5ba805825da76410c181273ba90b1";
    let pin = |sha: &str, version: &str| ActionPinOverride {
        sha: sha.to_owned(),
        version: version.to_owned(),
    };
    let good = ActionsConfig {
        overrides: BTreeMap::from([("actions/checkout".to_owned(), pin(sha, "v7.0.1"))]),
    };
    assert_eq!(good.validate("cfg"), Ok(()));
    assert_eq!(ActionsConfig::default().validate("cfg"), Ok(()));
    let alint = ActionsConfig {
        overrides: BTreeMap::from([(
            "asamarts/alint".to_owned(),
            pin("9f9d34ba0eae3888299b9e570f43338b0e7f2cdb", "v0.16.1"),
        )]),
    };
    // Alint pin is policy-owned, not overridable (version-policy.md §2, GitHub Action defaults).
    assert!(matches!(
        alint.validate("cfg"),
        Err(ContractError::Config { problem, .. }) if problem == "unknown_action"
    ));
    for (action, override_pin, problem) in [
        ("bogus/action", pin(sha, "v1.2.3"), "unknown_action"),
        (
            "actions/checkout",
            pin("abc123", "v7.0.1"),
            "ref_must_be_full_sha",
        ),
        ("actions/checkout", pin(sha, "v7"), "invalid_version:v7"),
    ] {
        let config = ActionsConfig {
            overrides: BTreeMap::from([(action.to_owned(), override_pin)]),
        };
        let Err(velnor_actions_contract::ContractError::Config {
            key_path,
            problem: got,
            ..
        }) = config.validate("cfg")
        else {
            panic!("override {action} must be rejected");
        };
        assert_eq!(key_path, format!("actions.overrides.{action}"));
        assert_eq!(got, problem);
    }
}

#[test]
fn manifest_schemas_validate_and_lookup_targets() -> Result<(), ContractError> {
    let sha = "ab".repeat(32);
    let manifest = ReleaseManifest {
        schema: 1,
        version: "0.1.0".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        commit: "ab".repeat(20),
        targets: SUPPORTED_TARGETS
            .iter()
            .map(|target| TargetRecord {
                target: (*target).to_owned(),
                artifact: format!(
                    "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/{}",
                    asset_filename("0.1.0", target)
                ),
                sha256: sha.clone(),
            })
            .collect(),
    };
    manifest.validate("release.toml")?;
    assert!(
        manifest
            .record_for_target("x86_64-unknown-linux-gnu")
            .is_some()
    );
    assert!(manifest.record_for_target("other").is_none());
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "ab".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.com/velnor-actions-0.1.0".to_owned(),
                sha256: sha,
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2025.1.0".to_owned(),
            artifact: "https://example.com/mise-2025.1.0".to_owned(),
            sha256: "cd".repeat(32),
        },
        actions: vec![ActionPin {
            name: "actions/checkout".to_owned(),
            version: "v4.2.2".to_owned(),
            sha: "ab".repeat(20),
            reviewed: "2026-01-15".to_owned(),
        }],
    };
    lock.validate("generator.lock")?;
    assert!(lock.binary_for_target("x86_64-unknown-linux-gnu").is_some());
    let mut bad = lock.clone();
    bad.actions[0].sha = "xyz".to_owned();
    assert!(bad.validate("generator.lock").is_err());
    for commit in [
        String::new(),
        "xyz".to_owned(),
        "A".repeat(40),
        "c".repeat(39),
    ] {
        let mut bad = lock.clone();
        bad.generator.commit = commit;
        let err = bad
            .validate("generator.lock")
            .expect_err("lock commit shares manifest strictness");
        assert!(err.to_string().contains("malformed_commit"), "{err}");
    }
    Ok(())
}

#[test]
fn removed_rust_custom_tasks_configuration_is_rejected() {
    let stack = velnor_actions_contract::RustStackConfig::default_config();
    let mut value = serde_json::to_value(stack).expect("serialize Rust config");
    value
        .as_object_mut()
        .expect("config object")
        .insert("custom_tasks".to_owned(), serde_json::json!(["audit"]));
    assert!(serde_json::from_value::<velnor_actions_contract::RustStackConfig>(value).is_err());
}

#[test]
fn rust_target_and_features_reject_shell_expressions() {
    use velnor_actions_contract::{ContractError, RustStackConfig};
    let file = ".velnor/config.toml";
    let stack = RustStackConfig::default_config();
    let mut good = stack.clone();
    good.configurations[0].target = "x86_64-unknown-linux-gnu".to_owned();
    good.configurations[0].features = vec![
        "default".to_owned(),
        "serde/std".to_owned(),
        "dep:tokio".to_owned(),
    ];
    assert_eq!(good.validate(file), Ok(()));
    for bad in ["", "x86_64;evil", "${{secrets.x}}", "$(evil)", "a/b/c"] {
        let mut named = stack.clone();
        named.configurations[0].target = bad.to_owned();
        let Err(ContractError::Config {
            key_path, problem, ..
        }) = named.validate(file)
        else {
            panic!("{bad:?} target must fail");
        };
        assert_eq!(key_path, "stacks.rust.configurations.target");
        assert!(problem.starts_with("bad_target:"), "got {problem}");
    }
    for bad in ["", "feat;evil", "${{secrets.x}}", "$(evil)", "a b"] {
        let mut named = stack.clone();
        named.configurations[0].features = vec![bad.to_owned()];
        let Err(ContractError::Config {
            key_path, problem, ..
        }) = named.validate(file)
        else {
            panic!("{bad:?} feature must fail");
        };
        assert_eq!(key_path, "stacks.rust.configurations.features");
        assert!(problem.starts_with("bad_feature:"), "got {problem}");
    }
}
