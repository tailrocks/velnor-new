//! Execute gate: staged identities fail closed in boundary order.
use std::collections::BTreeMap;

use tempfile::TempDir;
use velnor_actions_contract::plan_id_for_run;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_runtime_execute::execute::{CHECK_ID_ENV, execute_check_to};

const TASK: &str = "stack/rust/demo/clippy/default";
const JOB: &str = "crate_clippy";
const BOUND: u64 = u64::MAX;

/// Stage one `plan.json` under a fake runner temp.
fn stage(text: &str) -> TempDir {
    stage_at("local", text)
}

fn stage_at(run_key: &str, text: &str) -> TempDir {
    let dir = TempDir::new().expect("temp");
    let run = dir.path().join("velnor").join(run_key);
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), text).expect("plan");
    dir
}

/// Write `.velnor/config.toml` under a fresh temp root.
fn rooted(body: &str) -> TempDir {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    std::fs::create_dir_all(&dir).expect("velnor dir");
    std::fs::write(dir.join("config.toml"), body).expect("config write");
    root
}

fn entry_for(task_id: &str, kind: &str, seed: u8, job_id: &str) -> (MatrixEntry, String) {
    let task_digest = format!("b3-{}", format!("{seed:02x}").repeat(32));
    let entry = MatrixEntry::derive(
        "rust",
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.to_owned()))]),
        },
        &format!("b3-{}", format!("{:02x}", seed + 10).repeat(32)),
        "local",
        job_id,
    )
    .expect("entry derives");
    (entry, task_digest)
}

/// Valid single-task fixture plan on `crate_clippy`.
fn fixture_plan(event: WorkflowEvent) -> Plan {
    let (entry, task_digest) = entry_for(TASK, "clippy", 1, JOB);
    let plan = Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "a".repeat(40),
        event,
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
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest,
            input_digest: format!("b3-{}", "02".repeat(32)),
            closure_digest: format!("b3-{}", "03".repeat(32)),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),

        artifact_tasks: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    plan
}

fn valid_text(event: WorkflowEvent) -> String {
    serde_json::to_string(&fixture_plan(event)).expect("plan json")
}

fn check_toml(id: &str, timeout_minutes: u32) -> String {
    format!(
        "schema = 1\n[[checks]]\nid = \"{id}\"\ntask = \"check\"\ndirectory = \".\"\ninputs = [\"mise.toml\"]\ntools = []\ntimeout_minutes = {timeout_minutes}\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
    )
}

#[test]
fn missing_plan_reports_not_found() {
    let root = TempDir::new().expect("root");
    let temp = TempDir::new().expect("temp");
    let err = execute_check_to(
        root.path(),
        temp.path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("missing plan refuses");
    assert!(err.to_string().contains("not_found"), "{err}");
}

#[test]
fn garbage_plan_is_unparsable() {
    let root = TempDir::new().expect("root");
    let err = execute_check_to(
        root.path(),
        stage("{nope").path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("garbage refuses");
    assert!(err.to_string().contains("unparsable_plan"), "{err}");
}

#[test]
fn foreign_run_key_mismatches() {
    let root = TempDir::new().expect("root");
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage_at("other", &text).path(),
        "other",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("foreign run key refuses");
    assert!(err.to_string().contains("report_run_mismatch"), "{err}");
}

#[test]
fn plan_bound_is_honored() {
    let root = TempDir::new().expect("root");
    let text = valid_text(WorkflowEvent::PullRequest);
    let bound = u64::try_from(text.len() - 1).expect("bound fits");
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        bound,
    )
    .expect_err("oversize refuses");
    assert!(err.to_string().contains("oversize"), "{err}");
}

#[test]
fn missing_config_is_config_missing() {
    let root = TempDir::new().expect("root");
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("missing config refuses");
    assert!(
        matches!(err, OrchestratorError::ConfigMissing { .. }),
        "{err}"
    );
}

#[test]
fn garbage_config_names_its_file() {
    let root = rooted("{nope");
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("garbage config refuses");
    assert!(err.to_string().contains(".velnor/config.toml"), "{err}");
}

#[test]
fn unknown_check_is_not_configured() {
    let root = rooted("schema = 1\n");
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("unknown check refuses");
    assert!(err.to_string().contains("check_not_configured"), "{err}");
}

#[test]
fn unrelated_check_list_still_not_configured() {
    let root = rooted(&check_toml("other", 20));
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("unrelated list refuses");
    assert!(err.to_string().contains("check_not_configured"), "{err}");
}

#[test]
fn absurd_timeout_rejected_at_config() {
    let root = rooted(&check_toml("demo", u32::MAX));
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("absurd timeout refuses");
    assert!(err.to_string().contains("invalid_check_timeout"), "{err}");
}

#[test]
fn non_local_event_requires_git_checkout() {
    let root = rooted(&check_toml("demo", 20));
    let text = valid_text(WorkflowEvent::PullRequest);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("bare root refuses checkout");
    assert!(err.to_string().contains("bad_checkout"), "{err}");
}

#[test]
fn local_event_reaches_discovery() {
    let root = rooted(&check_toml("demo", 20));
    let text = valid_text(WorkflowEvent::Local);
    let err = execute_check_to(
        root.path(),
        stage(&text).path(),
        "local",
        "demo",
        TASK,
        JOB,
        "single",
        BOUND,
    )
    .expect_err("bare root cannot discover");
    assert!(
        err.to_string().contains("exactly_one_task_config_required"),
        "{err}"
    );
}

#[test]
fn check_id_env_pins_lookup_key() {
    assert_eq!(CHECK_ID_ENV, "VELNOR_CHECK_ID");
}
