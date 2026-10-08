//! Mise setup pin resolution.

use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
use velnor_actions_contract_config::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract_config::{
    DiscoveryConfig, GeneratorValidation, ResourcesConfig, StacksConfig, TestShardingConfig,
    VelnorConfig, VerificationRunner, WorkflowConfig, WorkflowPolicy,
};
use velnor_actions_orchestrator_pins::pins::{resolve_mise_setup, resolve_verification_mise_setup};

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
            tasks: Vec::new(),
            artifact_tasks: Vec::new(),
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
        docs: None,
    }
}

#[test]
fn default_setup_uses_compiled_pin() {
    let setup = resolve_mise_setup(&config_with(BTreeMap::new()), "ubuntu-26.04").expect("setup");
    assert_eq!(setup.uses, format!("jdx/mise-action@{MISE_ACTION_SHA}"));
    assert!(!setup.version.is_empty());
    assert_eq!(setup.sha256.len(), 64);
}

#[test]
fn approved_override_is_accepted() {
    let overrides = BTreeMap::from([(
        "jdx/mise-action".to_owned(),
        ActionPinOverride {
            sha: MISE_ACTION_SHA.to_owned(),
            version: MISE_ACTION_VERSION.to_owned(),
        },
    )]);
    let setup = resolve_mise_setup(&config_with(overrides), "ubuntu-26.04").expect("setup");
    assert!(setup.uses.ends_with(MISE_ACTION_SHA));
}

#[test]
fn unapproved_override_fails_closed() {
    let overrides = BTreeMap::from([(
        "jdx/mise-action".to_owned(),
        ActionPinOverride {
            sha: "0".repeat(40),
            version: MISE_ACTION_VERSION.to_owned(),
        },
    )]);
    resolve_mise_setup(&config_with(overrides), "ubuntu-26.04").expect_err("unapproved");
}

#[test]
fn unsupported_runner_fails_closed() {
    let err = resolve_mise_setup(&config_with(BTreeMap::new()), "windows-latest")
        .expect_err("bad target");
    assert!(
        err.to_string().contains("mise_setup_unsupported_target"),
        "{err}"
    );
}

#[test]
fn verification_setup_selects_runner_sha() {
    let config = config_with(BTreeMap::new());
    let linux =
        resolve_verification_mise_setup(&config, VerificationRunner::LinuxX64).expect("linux");
    let macos =
        resolve_verification_mise_setup(&config, VerificationRunner::MacosArm64).expect("macos");
    assert_eq!(linux.sha256.len(), 64);
    assert_eq!(macos.sha256.len(), 64);
    assert_ne!(linux.sha256, macos.sha256);
    assert_eq!(linux.version, macos.version);
}
