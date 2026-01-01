//! Config/plan fixtures shared by this target's remediation cases.
//!
//! Duplicated per contract-family test target at the SIZE split (sibling
//! copies in the `contract-config`, `contract-workflow`, and
//! `contract-planning` test targets): each integration target links
//! alone, so shared helpers ride per target. Keep the copies in sync;
//! drift fails the agreement pins.

use crate::impl_contract_ids::{MANIFEST, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::{ContractError, digest_b3, plan_id_for_run};
use velnor_actions_contract_config::config::{
    ActionsConfig, DiscoveryConfig, ResourcesConfig, RunnerSelection, StacksConfig,
    TestShardingConfig, VelnorConfig, VerifyConfig, WorkflowConfig, WorkflowPolicy,
};
use velnor_actions_contract_workflow::workflow::{
    ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage,
    PlanRunner, Trust, WorkflowEvent,
};

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
            artifact_tasks: Vec::new(),
            verify: VerifyConfig::default(),
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

        artifact_tasks: Vec::new(),
    })
}
