//! Outcome persistence: receipts, evidence bytes, and refusal cases.
use std::collections::BTreeMap;

use tempfile::TempDir;
use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_config::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck,
};
use velnor_actions_contract_planning::{
    CachePolicy, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
};
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_mise::DiscoveredCheck;
use velnor_actions_mise::checks::CheckCapabilityProof;
use velnor_actions_orchestrator_check_evidence::scenario::EvidenceReceipt;
use velnor_actions_orchestrator_check_preparation::container_receipts::ContainerReceipt;
use velnor_actions_orchestrator_runtime_evidence::evidence::{
    CheckOutcome, save_evidence, write_execution_receipt,
};

const TASK: &str = "stack/mise/demo/check/default";
const JOB: &str = "check-demo";

/// Bare plan carrying the run key and head the writers persist.
fn plan() -> Plan {
    Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "test-plan".to_owned(),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "test".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "test".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: format!("b3-{}", "01".repeat(32)),
            input_digest: format!("b3-{}", "02".repeat(32)),
            closure_digest: format!("b3-{}", "03".repeat(32)),
            baseline_proof: None,
        }],
        matrix: PlanMatrix { include: vec![] },
        task_ids: vec![TASK.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),

        artifact_tasks: Vec::new(),
    }
}

fn entry() -> MatrixEntry {
    let digest = format!("b3-{}", "01".repeat(32));
    MatrixEntry::derive(
        "mise",
        TASK,
        "true",
        &digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([("check".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()))]),
        },
        &format!("b3-{}", "02".repeat(32)),
        "local",
        JOB,
    )
    .expect("entry derives")
}

fn evidence_receipt(path: &str, scenarios: Vec<String>) -> EvidenceReceipt {
    EvidenceReceipt {
        schema: 1,
        check_id: "demo".to_owned(),
        source: "mise-task-v1".to_owned(),
        head: "HEAD".to_owned(),
        platform: CheckPlatform::LinuxX64,
        path: path.to_owned(),
        digest: "b3-test".to_owned(),
        scenarios,
        bytes: b"{}".to_vec(),
    }
}

fn container_receipt() -> ContainerReceipt {
    let proof = CheckCapabilityProof { container: None };
    ContainerReceipt {
        profile_digest: "profile".to_owned(),
        before: proof.clone(),
        after: proof,
        sdk: None,
        runtime: serde_json::Value::Null,
        before_runtime: None,
        after_runtime: None,
    }
}

fn outcome() -> CheckOutcome {
    CheckOutcome {
        evidence: Some(evidence_receipt("proof.json", vec!["one".to_owned()])),
        container: None,
        system_tools: Vec::new(),
        qualified_tools: Vec::new(),
    }
}

fn discovered() -> DiscoveredCheck {
    DiscoveredCheck {
        check: MiseCheck {
            id: "demo".to_owned(),
            task: "check".to_owned(),
            directory: ".".to_owned(),
            runner: CheckRunner {
                label: "test".to_owned(),
                platform: CheckPlatform::LinuxX64,
                executor: CheckExecutor::Hosted,
                container: None,
            },
            inputs: Vec::new(),
            tools: Vec::new(),
            system_tools: Vec::new(),
            evidence: None,
            timeout_minutes: 30,
        },
        proposal: ProposedTask {
            task_id: TASK.to_owned(),
            stack_id: "mise".to_owned(),
            component_id: "demo".to_owned(),
            task_kind: "check".to_owned(),
            configuration: "default".to_owned(),
            depends_on: Vec::new(),
            gated_by: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            outputs: Vec::new(),
            resource: ResourceDemand {
                class: ResourceClass::Lightweight,
                cpu_milli: None,
                memory_mb: None,
                needs_network: false,
                service: None,
            },
            cache_policy: CachePolicy {
                allow_compilation_reuse: false,
                allow_task_reuse: false,
            },
            identity: IdentityInputs {
                unit_id: "demo".to_owned(),
                unit_key: "root".to_owned(),
                unit_path: "mise.toml".to_owned(),
                project_root: ".".to_owned(),
                target: "host".to_owned(),
                features: Vec::new(),
                flags: Vec::new(),
                compile_driver: "mise".to_owned(),
                test_runner: "mise".to_owned(),
                environment: BTreeMap::new(),
                declared_inputs: Vec::new(),
                undeclared_reads: false,
            },
            payload: Vec::new(),
            display_name: "demo".to_owned(),
            uses_clock: false,
            uses_random: false,
            no_targets: false,
            runner_profile: "default".to_owned(),
        },
        config_inputs: Vec::new(),
        task_config: String::new(),
        tool_specs: Vec::new(),
        qualified_tools: Vec::new(),
        qualification_digest: String::new(),
        config_source: String::new(),
    }
}

fn staged_home(temp: &TempDir, plan: &Plan, entry: &MatrixEntry) -> std::path::PathBuf {
    temp.path()
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key)
}

#[test]
fn writes_nested_evidence_file() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let receipt = evidence_receipt("proof.json", vec!["one".to_owned()]);
    save_evidence(temp.path(), &plan, &entry, &receipt).expect("evidence writes");
    let bytes = std::fs::read(staged_home(&temp, &plan, &entry).join("evidence/proof.json"))
        .expect("evidence file");
    assert_eq!(bytes, b"{}");
}

#[test]
fn deep_evidence_path_creates_parents() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let receipt = evidence_receipt("nested/deep.json", vec!["one".to_owned()]);
    save_evidence(temp.path(), &plan, &entry, &receipt).expect("deep evidence writes");
    assert!(
        staged_home(&temp, &plan, &entry)
            .join("evidence/nested/deep.json")
            .is_file()
    );
}

#[test]
fn refuses_preexisting_evidence() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let receipt = evidence_receipt("proof.json", vec!["one".to_owned()]);
    save_evidence(temp.path(), &plan, &entry, &receipt).expect("first write wins");
    assert!(
        save_evidence(temp.path(), &plan, &entry, &receipt).is_err(),
        "second write refuses"
    );
}

#[test]
fn writes_check_execution_json() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &outcome())
        .expect("receipt writes");
    let bytes = std::fs::read(staged_home(&temp, &plan, &entry).join("check-execution.json"))
        .expect("receipt file");
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert_eq!(receipt["check_id"], "demo");
    assert_eq!(receipt["task_id"], TASK);
    assert_eq!(receipt["evidence"]["scenarios"], serde_json::json!(["one"]));
}

#[test]
fn receipt_omits_missing_evidence() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let mut bare = outcome();
    bare.evidence = None;
    write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &bare)
        .expect("bare receipt writes");
    let bytes = std::fs::read(staged_home(&temp, &plan, &entry).join("check-execution.json"))
        .expect("receipt file");
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert!(receipt.get("evidence").is_none_or(|value| value.is_null()));
}

#[test]
fn receipt_carries_container_observation() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let mut contained = outcome();
    contained.container = Some(container_receipt());
    write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &contained)
        .expect("contained receipt writes");
    let bytes = std::fs::read(staged_home(&temp, &plan, &entry).join("check-execution.json"))
        .expect("receipt file");
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert_eq!(receipt["container"]["profile_digest"], "profile");
}

#[test]
fn receipt_omits_missing_container() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &outcome())
        .expect("receipt writes");
    let bytes = std::fs::read(staged_home(&temp, &plan, &entry).join("check-execution.json"))
        .expect("receipt file");
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert!(receipt.get("container").is_none_or(|value| value.is_null()));
}

#[test]
fn oversize_receipt_refuses() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let mut huge = outcome();
    huge.evidence = Some(evidence_receipt("proof.json", vec!["x".repeat(1 << 20)]));
    let err = write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &huge)
        .expect_err("oversize refuses");
    assert!(
        err.to_string()
            .contains("check_execution_receipt_size_limit"),
        "{err}"
    );
}

#[test]
fn refuses_preexisting_receipt() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    let item = discovered();
    let written = outcome();
    write_execution_receipt(temp.path(), &plan, &entry, &item, &written).expect("first write wins");
    assert!(
        write_execution_receipt(temp.path(), &plan, &entry, &item, &outcome()).is_err(),
        "second write refuses"
    );
}

#[test]
fn receipt_scopes_to_run_and_matrix() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &outcome())
        .expect("receipt writes");
    let expected = temp
        .path()
        .join("velnor")
        .join("local")
        .join(&entry.matrix_key)
        .join("check-execution.json");
    assert!(expected.is_file());
}

#[test]
fn receipt_is_canonical_json() {
    let temp = TempDir::new().expect("temp");
    let plan = plan();
    let entry = entry();
    write_execution_receipt(temp.path(), &plan, &entry, &discovered(), &outcome())
        .expect("receipt writes");
    let bytes = std::fs::read(staged_home(&temp, &plan, &entry).join("check-execution.json"))
        .expect("receipt file");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert_eq!(canonical_json_bytes(&value).expect("canonical"), bytes);
}
