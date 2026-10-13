//! Fixture vocabulary shared by this target's planning cases.
//!
//! Duplicated per contract-family test target at the SIZE split (sibling
//! copies in the `contract`, `contract-config`, and
//! `contract-workflow` test targets): each integration target links
//! alone, so shared helpers ride per target. Keep the copies in sync;
//! drift fails the agreement pins.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    ContractError, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput,
    VcsInputs, digest_b3, plan_id_for_run,
};
use velnor_actions_contract_config::config::{
    ActionsConfig, DiscoveryConfig, ResourcesConfig, RunnerSelection, StacksConfig,
    TestShardingConfig, VelnorConfig, WorkflowConfig, WorkflowPolicy,
};
use velnor_actions_contract_workflow::workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner, Trust, WorkflowEvent,
};

/// Sample Cargo manifest path shared by contract cases.
pub(crate) const MANIFEST: &str = "crates/core/velnor-actions-contract/Cargo.toml";
/// Sample task ID shared by contract cases.
pub(crate) const TASK: &str = "stack/rust/crates/core/velnor-actions-contract/clippy/default";
/// Sample task-group ID shared by contract cases.
pub(crate) const GROUP: &str = "stack/rust/crates/core/velnor-actions-contract/validation/default";

/// Sample task identity shared by contract cases.
pub(crate) fn sample_identity() -> TaskIdentity {
    TaskIdentity {
        schema_version: 1,
        stack_id: "rust".to_owned(),
        project_root: ".".to_owned(),
        component_id: "crates/core/velnor-actions-contract".to_owned(),
        task_kind: "clippy".to_owned(),
        task_id: TASK.to_owned(),
        argv: vec![
            "clippy".to_owned(),
            "--package".to_owned(),
            "demo".to_owned(),
        ],
        working_dir: "crates/core/velnor-actions-contract".to_owned(),
        configuration: TaskConfiguration {
            target: "host".to_owned(),
            profile: "test".to_owned(),
            features: vec!["default".to_owned()],
            flags: vec![],
            task_contract: "clippy-v1".to_owned(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
        },
        inputs: vec![TaskInput {
            path: "crates/core/velnor-actions-contract/src/lib.rs".to_owned(),
            digest: digest_b3(b"fn main() {}"),
        }],
        dependencies: vec![],
        vcs: VcsInputs {
            commit: None,
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: digest_b3(b"toolchain"),
        platform_id: digest_b3(b"platform"),
        environment: BTreeMap::from([("RUSTFLAGS".to_owned(), "-D warnings".to_owned())]),
        output_contract: "clippy-report-v1".to_owned(),
        generator: TaskGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
        },
        stack_extension: StackExtension {
            schema: "rust-task-v1".to_owned(),
            data: serde_json::json!({"manifest": MANIFEST}),
        },
    }
}

/// Fixed leg command shared by contract matrix fixtures.
pub(crate) const SAMPLE_RUN: &str = "mise exec --no-config rust@1.98.1 -- cargo clippy --locked";

/// Build one sample matrix entry shared by contract cases.
pub(crate) fn sample_entry(run_key: &str) -> Result<MatrixEntry, ContractError> {
    let mut tasks = BTreeMap::new();
    tasks.insert("clippy".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()));
    MatrixEntry::derive(
        "rust",
        GROUP,
        SAMPLE_RUN,
        &digest_b3(b"task-bytes"),
        serde_json::json!({"manifest": MANIFEST}),
        ExecuteTaskIds { tasks },
        &digest_b3(b"entry-inputs"),
        run_key,
        "plan",
    )
}

/// Valid config shared by remediation cases.
pub(crate) fn valid_config() -> VelnorConfig {
    VelnorConfig {
        checks: Vec::new(),
        qualified_tools: Vec::new(),
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation:
                velnor_actions_contract_config::config::GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
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
            ignore: vec![],
            rust: None,
            tofu: None,
        },
        discovery: DiscoveryConfig { exclude: vec![] },
        actions: ActionsConfig::default(),
        execution: None,
        docs: None,
    }
}

/// Valid plan shared by remediation cases.
pub(crate) fn sample_plan(run_key: &str) -> Result<Plan, ContractError> {
    let entry = sample_entry(run_key)?;
    Ok(Plan {
        schema: 1,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key)?,
        base: None,
        head: "ab".repeat(20),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("no_entry"))?,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: MANIFEST.to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![TASK.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: digest_b3(b"task"),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
        edges: vec![],
    })
}
