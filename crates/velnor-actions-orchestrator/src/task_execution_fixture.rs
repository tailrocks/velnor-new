//! Shared task-execution resolver fixture construction.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform,
    TASK_EXECUTION_MANIFEST_PATH, TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1,
    TaskExecutionManifestV1, WorkflowEvent,
};
use velnor_actions_contract::{RunnerSelection, Trust, plan_id_for_run};

use super::super::{OrchestratorError, resolve_task_execution_to};
use super::support::digest;
use super::{GENERATOR_VERSION, RUN_KEY, TASK_ID};

pub(super) struct Fixture {
    pub(super) repo: TempDir,
    pub(super) runner_temp: TempDir,
    pub(super) manifest: TaskExecutionManifestV1,
    pub(super) plan: Plan,
}

fn manifest_and_task_digest() -> (TaskExecutionManifestV1, String) {
    let argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        "rust@1.99.0".to_owned(),
        "--".to_owned(),
        "cargo".to_owned(),
        "fmt".to_owned(),
        "--check".to_owned(),
    ];
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain id");
    let task_digest = task_digest_for_execution(TASK_ID, &argv, &toolchain).expect("task digest");
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group("rust", TASK_ID).expect("matrix id");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.99.0".to_owned()),
        ("RUSTDOCFLAGS".to_owned(), "-D warnings".to_owned()),
    ]);
    let manifest = build_manifest(
        argv,
        toolchain_inputs,
        task_digest.clone(),
        env,
        matrix_id,
        matrix_key,
    );
    (manifest, task_digest)
}

fn build_manifest(
    argv: Vec<String>,
    toolchain_inputs: ToolchainInputs,
    task_digest: String,
    env: BTreeMap<String, String>,
    matrix_id: String,
    matrix_key: String,
) -> TaskExecutionManifestV1 {
    let mut record = TaskExecutionManifestEntryV1 {
        task_id: TASK_ID.to_owned(),
        execution_digest: String::new(),
        task_digest,
        toolchain_inputs,
        argv,
        env,
        matrix_id,
        matrix_key,
        report_helper_version: GENERATOR_VERSION.to_owned(),
        matrix_max_parallel: Some(8),
    };
    record.refresh_execution_digest().expect("execution digest");
    let manifest = TaskExecutionManifestV1 {
        schema: TASK_EXECUTION_MANIFEST_SCHEMA,
        generator_version: GENERATOR_VERSION.to_owned(),
        tasks: BTreeMap::from([(TASK_ID.to_owned(), record)]),
    };
    manifest.validate().expect("manifest validates");
    manifest
}

fn build_plan(task_digest: String) -> Plan {
    let entry = MatrixEntry::derive(
        "rust",
        TASK_ID,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "clippy".to_owned(),
                ExecuteTaskRef::Single(TASK_ID.to_owned()),
            )]),
        },
        &digest(11),
        RUN_KEY,
        "crate_clippy",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("matrix entry");
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: RUN_KEY.to_owned(),
        plan_id: plan_id_for_run(RUN_KEY).expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: GENERATOR_VERSION.to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: TASK_ID.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest,
            input_digest: digest(11),
            closure_digest: digest(12),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK_ID.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("plan validates");
    plan
}

fn write_fixture_files(
    repo: &TempDir,
    runner_temp: &TempDir,
    manifest: &TaskExecutionManifestV1,
    plan: &Plan,
) {
    let manifest_path = repo.path().join(TASK_EXECUTION_MANIFEST_PATH);
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("manifest directory");
    fs::write(
        &manifest_path,
        manifest.marked_json().expect("marked manifest"),
    )
    .expect("manifest file");
    let plan_path = runner_temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join("plan.json");
    fs::create_dir_all(plan_path.parent().expect("plan parent")).expect("plan directory");
    fs::write(&plan_path, serde_json::to_vec(plan).expect("plan JSON")).expect("plan file");
}

impl Fixture {
    pub(super) fn new() -> Self {
        let (manifest, task_digest) = manifest_and_task_digest();
        let plan = build_plan(task_digest);
        let repo = TempDir::new().expect("repo tempdir");
        let runner_temp = TempDir::new().expect("runner tempdir");
        write_fixture_files(&repo, &runner_temp, &manifest, &plan);
        Self {
            repo,
            runner_temp,
            manifest,
            plan,
        }
    }

    pub(super) fn resolve(&self) -> Result<Vec<u8>, OrchestratorError> {
        let record = self.manifest.tasks.get(TASK_ID).expect("manifest record");
        resolve_task_execution_to(
            self.repo.path(),
            self.runner_temp.path(),
            RUN_KEY,
            &record.execution_digest,
            GENERATOR_VERSION,
        )
    }
}
