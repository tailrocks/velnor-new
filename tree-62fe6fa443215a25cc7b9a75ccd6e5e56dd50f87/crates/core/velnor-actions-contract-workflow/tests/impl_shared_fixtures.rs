//! Fixture vocabulary shared by this target's workflow cases.
//!
//! Duplicated per contract-family test target at the SIZE split (sibling
//! copies in the `contract`, `contract-config`, and
//! `contract-planning` test targets): each integration target links
//! alone, so shared helpers ride per target. Keep the copies in sync;
//! drift fails the agreement pins.

use std::collections::BTreeMap;

use velnor_actions_contract::{ContractError, digest_b3, plan_id_for_run};
use velnor_actions_contract_config::config::RunnerSelection;
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
