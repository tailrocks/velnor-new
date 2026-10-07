use super::super::*;
use crate::check_evidence::{gate::execution_receipt, verify_evidence};
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;
use velnor_actions_contract::{canonical_json_bytes, plan_id_for_run};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_config::config::{
    CheckEvidence, CheckPlatform, HostContainerProfile, QualifiedTool,
};
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, MatrixReport, ObligationDecision, Plan,
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, TaskReport, Trust,
    WorkflowEvent, matrix_json_bytes,
};

/// Clippy fixture task ID.
pub(super) const CLIPPY: &str = "stack/rust/demo/clippy/default";

/// Test fixture task ID.
pub(super) const TEST: &str = "stack/rust/demo/test/default";

/// One single-task plan entry plus its obligation digest.
pub(super) fn entry_for(
    stack: &str,
    task_id: &str,
    kind: &str,
    seed: u8,
    job_id: &str,
) -> (MatrixEntry, String) {
    let task_digest = digest(seed);
    let entry = MatrixEntry::derive(
        stack,
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.to_owned()))]),
        },
        &digest(seed + 10),
        "local",
        job_id,
    )
    .expect("entry derives");
    (entry, task_digest)
}

/// Valid two-task fixture plan (clippy plus test).
pub(super) fn fixture_plan() -> Plan {
    let (clippy_entry, clippy_digest) = entry_for("rust", CLIPPY, "clippy", 1, "crate_clippy");
    let (test_entry, test_digest) = entry_for("rust", TEST, "test", 2, "crate_test");
    let plan = Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![
            obligation_for(CLIPPY, clippy_digest, 1),
            obligation_for(TEST, test_digest, 2),
        ],
        matrix: PlanMatrix {
            include: vec![clippy_entry, test_entry],
        },
        task_ids: vec![CLIPPY.to_owned(), TEST.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    plan
}

/// One execute obligation.
pub(super) fn obligation_for(task_id: &str, task_digest: String, seed: u8) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest,
        input_digest: digest(seed + 10),
        closure_digest: digest(seed + 20),
        baseline_proof: None,
    }
}

/// Read and validate one staged task report plus its aggregate.
pub(super) fn read_entry(
    temp: &TempDir,
    run_key: &str,
    matrix_key: &str,
    task_report_id: &str,
) -> (TaskReport, MatrixReport) {
    let dir = temp.path().join("velnor").join(run_key).join(matrix_key);
    let matrix_bytes = fs::read(dir.join("matrix-report.json")).expect("matrix file");
    let task_bytes =
        fs::read(dir.join("tasks").join(format!("{task_report_id}.json"))).expect("task file");
    let matrix: MatrixReport = serde_json::from_slice(&matrix_bytes).expect("matrix json");
    let task: TaskReport = serde_json::from_slice(&task_bytes).expect("task json");
    matrix.validate().expect("matrix validates");
    task.validate().expect("task validates");
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&matrix).expect("canonical matrix"),
        matrix_bytes,
        "matrix bytes are canonical Rust output"
    );
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&task).expect("canonical task"),
        task_bytes,
        "task bytes are canonical Rust output"
    );
    (task, matrix)
}

/// Staged run dir: `velnor/<run-key>/plan.json` plus `matrix.json`.
pub(super) fn staged_run(plan: &Plan, run_key: &str) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let dir = temp.path().join("velnor").join(run_key);
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(
        dir.join("plan.json"),
        serde_json::to_string(plan).expect("plan json"),
    )
    .expect("plan file");
    fs::write(
        dir.join("matrix.json"),
        matrix_json_bytes(&plan.matrix).expect("matrix json"),
    )
    .expect("matrix file");
    temp
}

pub(super) fn staged_with_envelope(
    with_proof: bool,
    declarations: &[QualifiedTool],
    qualified_tools: &[velnor_actions_orchestrator_check_acquisition::tools::QualifiedToolReceipt],
    container_profile: Option<&HostContainerProfile>,
    container: Option<
        &velnor_actions_orchestrator_check_preparation::container_receipts::ContainerReceipt,
    >,
) -> (tempfile::TempDir, Plan) {
    let temp = tempfile::TempDir::new().expect("temp");
    let mut plan = plan_with_tools(declarations);
    if let Some(profile) = container_profile {
        plan.matrix.include[0].adapter_metadata["runner"] = serde_json::json!({
            "label":"native-scale",
            "platform":"linux_x64",
            "executor":"ephemeral_self_hosted",
            "container":profile,
        });
        plan.validate().expect("container runner metadata");
    }
    let entry = &plan.matrix.include[0];
    fs::write(temp.path().join("proof.json"), producer_json(&plan)).expect("producer evidence");
    let receipt = verify_evidence(
        temp.path(),
        &CheckEvidence {
            path: "proof.json".into(),
            expected_scenarios: vec!["one".into()],
        },
        "demo",
        &plan.head,
        CheckPlatform::LinuxX64,
    )
    .expect("verified evidence");
    let run = temp.path().join("velnor/local");
    fs::create_dir_all(&run).expect("run");
    fs::write(
        run.join("plan.json"),
        canonical_json_bytes(&plan).expect("plan bytes"),
    )
    .expect("plan");
    fs::write(
        run.join("matrix.json"),
        canonical_json_bytes(&plan.matrix).expect("matrix bytes"),
    )
    .expect("matrix");
    let mut task =
        terminal_task_report(&plan, entry, &entry.task_digest, 0, Some(1)).expect("task");
    task.outputs = vec!["proof.json".into()];
    let matrix = single_task_aggregate(&plan, entry, &task).expect("matrix");
    write_entry_reports(temp.path(), &plan, entry, &task, &matrix).expect("reports");
    let home = run.join(&entry.matrix_key);
    if with_proof {
        fs::create_dir(home.join("evidence")).expect("evidence dir");
        fs::write(home.join("evidence/proof.json"), &receipt.bytes).expect("evidence artifact");
        let mut execution = execution_receipt(
            &plan,
            entry,
            "demo",
            CheckPlatform::LinuxX64,
            Some(receipt),
            vec![],
            qualified_tools.to_vec(),
        );
        execution.container = container.cloned();
        fs::write(
            home.join("check-execution.json"),
            canonical_json_bytes(&execution).expect("receipt bytes"),
        )
        .expect("receipt");
    }
    let downloads = run.join("reports").join(&entry.artifact_id);
    fs::create_dir_all(&downloads).expect("downloads");
    fs::rename(home, downloads.join(&entry.matrix_key)).expect("stage artifact");
    (temp, plan)
}

pub(super) fn plan_with_tools(qualified_tools: &[QualifiedTool]) -> Plan {
    let mut plan = fixture_plan();
    let mut obligation = plan.obligations.remove(0);
    obligation.task_id = TASK.into();
    let mut tool_specs: Vec<_> = qualified_tools
        .iter()
        .filter_map(|tool| match &tool.backend {
            velnor_actions_contract_config::config::QualifiedToolBackend::Aqua { package } => {
                Some(format!("aqua:{package}@{}", tool.version))
            }
            _ => None,
        })
        .collect();
    tool_specs.sort();
    let qualification_digest =
        velnor_actions_mise::checks::DiscoveredCheck::qualification_fingerprint(
            qualified_tools,
            &tool_specs,
        )
        .expect("fingerprint");
    let mut entry = MatrixEntry::derive("mise", TASK, "true", &obligation.task_digest,
        serde_json::json!({"check_id":"demo","system_tools":[],"qualified_tools":qualified_tools,"tool_specs":tool_specs,"qualification_digest":qualification_digest,"evidence":{"path":"proof.json","expected_scenarios":["one"]},
            "runner":{"label":"ubuntu-24.04","platform":"linux_x64","executor":"hosted"}}),
        ExecuteTaskIds { tasks: BTreeMap::from([("check".into(), ExecuteTaskRef::Single(TASK.into()))]) },
        &obligation.input_digest, "local", "check-demo").expect("entry");
    entry.declared_outputs = vec!["proof.json".into()];
    plan.matrix.include = vec![entry];
    plan.obligations = vec![obligation];
    plan.task_ids = vec![TASK.into()];
    plan.packages.clear();
    plan.edges.clear();
    plan.validate().expect("plan");
    plan
}

/// One `b3-` digest with every byte set to `byte`.
fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

pub(super) const TASK: &str = "stack/mise/demo/check/default";

pub(super) fn producer_json(plan: &Plan) -> String {
    serde_json::json!({"schema":1,"source":"mise-task-v1","head":plan.head,"check_id":"demo","platform":"linux_x64",
        "scenarios":[{"id":"one","executed":true,"status":"passed"}]}).to_string()
}
