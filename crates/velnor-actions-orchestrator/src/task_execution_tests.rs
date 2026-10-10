//! Focused tests for the data-only generated-task resolver.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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

use super::*;

const RUN_KEY: &str = "local";
const TASK_ID: &str = "stack/rust/demo/clippy/default";
const GENERATOR_VERSION: &str = "0.1.7";

struct Fixture {
    repo: TempDir,
    runner_temp: TempDir,
    manifest: TaskExecutionManifestV1,
    plan: Plan,
}

impl Fixture {
    fn new() -> Self {
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
        let task_digest =
            task_digest_for_execution(TASK_ID, &argv, &toolchain).expect("task digest");
        let matrix_id =
            velnor_actions_contract::matrix_id_for_task_group("rust", TASK_ID).expect("matrix id");
        let matrix_key =
            velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
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
        let mut record = TaskExecutionManifestEntryV1 {
            task_id: TASK_ID.to_owned(),
            execution_digest: String::new(),
            task_digest: task_digest.clone(),
            toolchain_inputs,
            argv,
            env,
            matrix_id: matrix_id.clone(),
            matrix_key: matrix_key.clone(),
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
            PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")
                .expect("planned platform"),
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

        let repo = TempDir::new().expect("repo tempdir");
        let manifest_path = repo.path().join(TASK_EXECUTION_MANIFEST_PATH);
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("manifest directory");
        fs::write(
            &manifest_path,
            manifest.marked_json().expect("marked manifest"),
        )
        .expect("manifest file");
        let runner_temp = TempDir::new().expect("runner tempdir");
        let plan_path = runner_temp
            .path()
            .join("velnor")
            .join(RUN_KEY)
            .join("plan.json");
        fs::create_dir_all(plan_path.parent().expect("plan parent")).expect("plan directory");
        fs::write(&plan_path, serde_json::to_vec(&plan).expect("plan JSON")).expect("plan file");

        Self {
            repo,
            runner_temp,
            manifest,
            plan,
        }
    }

    fn resolve(&self) -> Result<Vec<u8>, OrchestratorError> {
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

#[test]
fn resolver_emits_the_bound_record_without_altering_plan_data() {
    let fixture = Fixture::new();
    let before = serde_json::to_vec(&fixture.plan).expect("plan before");
    let frame = fixture.resolve().expect("resolve execution data");
    let after = fs::read(
        fixture
            .runner_temp
            .path()
            .join("velnor")
            .join(RUN_KEY)
            .join("plan.json"),
    )
    .expect("plan after");
    assert_eq!(after, before, "the resolver is data-only and read-only");
    assert!(frame.len() <= MAX_TASK_EXECUTION_FRAME_BYTES);
    let fields = nul_fields(&frame);
    let record = fixture.manifest.tasks.get(TASK_ID).expect("record");
    assert_eq!(
        fields[0],
        velnor_actions_contract::TASK_EXECUTION_FRAME_MAGIC
    );
    assert_eq!(fields[1], TASK_ID);
    assert_eq!(fields[2], record.execution_digest);
    assert_eq!(fields[3], record.task_digest);
    assert_eq!(fields[4], record.matrix_id);
    assert_eq!(fields[5], record.matrix_key);
    assert_eq!(fields[6], record.report_helper_version);
    assert_eq!(fields.last(), Some(&"END"));
    assert!(fields.iter().all(|field| !field.contains("${{")));
    let argv_count = fields[9].parse::<usize>().expect("argv count");
    assert_eq!(
        &fields[10..10 + argv_count],
        record.argv.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let env_count_position = 10 + argv_count;
    let env_count = fields[env_count_position]
        .parse::<usize>()
        .expect("environment count");
    let env_start = env_count_position + 1;
    let resolved_env = fields[env_start..fields.len() - 1]
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(resolved_env.len(), env_count);
    let runner_temp = fixture
        .runner_temp
        .path()
        .to_str()
        .expect("UTF-8 runner temp");
    assert_eq!(
        resolved_env["MISE_RUSTUP_HOME"],
        format!("{runner_temp}/velnor/rustup")
    );
    assert_eq!(
        resolved_env["MISE_CARGO_HOME"],
        format!("{runner_temp}/velnor/cargo")
    );
    assert_eq!(resolved_env["RUSTDOCFLAGS"], "-D warnings");
    assert_eq!(
        record.execution_digest,
        record
            .computed_execution_digest()
            .expect("original execution digest")
    );
}

#[test]
fn resolver_rejects_unrecognized_template_expression() {
    let error =
        resolve_runner_temp_expression("${{ github.workspace }}/velnor/cargo", "/runner/_temp")
            .expect_err("unknown runtime template is rejected");
    assert_error(&error, "unsupported_task_execution_expression");
}

#[test]
fn resolver_rejects_unclosed_template_expression() {
    let error = resolve_runner_temp_expression("${{ runner.temp/velnor/cargo", "/runner/_temp")
        .expect_err("unclosed runtime template is rejected");
    assert_error(&error, "malformed_task_execution_expression");
}

#[test]
fn resolver_requires_runner_temp_to_match_the_generated_action_binding() {
    let valid = Path::new("/runner/_temp");
    validate_runner_temp_binding(valid, valid).expect("matching runner temp");

    let error = validate_runner_temp_binding(valid, Path::new("/other/_temp"))
        .expect_err("mismatched runner context rejected");
    assert_error(&error, "runner_temp_binding_mismatch");

    let error = validate_runner_temp_binding(valid, Path::new("relative/temp"))
        .expect_err("relative rendered value rejected");
    assert_error(&error, "runner_paths_must_be_absolute");
}

#[test]
fn resolver_rejects_untrusted_marker_version_before_manifest_selection() {
    let fixture = Fixture::new();
    let record = fixture.manifest.tasks.get(TASK_ID).expect("record");
    let error = resolve_task_execution_to(
        fixture.repo.path(),
        fixture.runner_temp.path(),
        RUN_KEY,
        &record.execution_digest,
        "0.1.7",
    )
    .expect_err("wrong static renderer version rejected");
    assert_error(&error, "task_execution_marker_mismatch");
}

#[test]
fn resolver_rejects_execution_and_plan_identity_mismatches() {
    let fixture = Fixture::new();
    let error = resolve_task_execution_to(
        fixture.repo.path(),
        fixture.runner_temp.path(),
        RUN_KEY,
        &digest(90),
        GENERATOR_VERSION,
    )
    .expect_err("wrong action execution digest rejected");
    assert_error(&error, "execution_digest_mismatch");

    let mut plan = fixture.plan.clone();
    let old_entry = plan.matrix.include[0].clone();
    plan.matrix.include[0] = MatrixEntry::derive(
        "tofu",
        TASK_ID,
        "true",
        &old_entry.task_digest,
        serde_json::json!({}),
        old_entry.execute_task_ids.clone(),
        &old_entry.input_digest,
        RUN_KEY,
        "crate_clippy",
        old_entry.planned_platform.clone(),
    )
    .expect("alternate but valid entry");
    plan.validate().expect("alternate plan validates");
    write_plan(fixture.runner_temp.path(), &plan);
    let error = fixture
        .resolve()
        .expect_err("matrix identity mismatch rejected");
    assert_error(&error, "task_execution_plan_binding_mismatch");
}

#[test]
fn resolver_rejects_ambiguous_execution_digest_matches() {
    let fixture = Fixture::new();
    let record = fixture.manifest.tasks.get(TASK_ID).expect("record");
    let expected_digest = record.execution_digest.as_str();
    let mut manifest = fixture.manifest.clone();

    // A second matching value stands in for a digest collision. The real
    // resolver validates every record before this selector runs; this direct
    // test proves the selector never chooses by BTreeMap iteration order.
    let mut collision = record.clone();
    collision.task_id.push_str("/collision");
    manifest.tasks.insert(collision.task_id.clone(), collision);

    let error = unique_record_by_execution_digest(&manifest, expected_digest)
        .expect_err("ambiguous digest match is rejected");
    assert_error(&error, "ambiguous_execution_digest");
}

#[test]
fn resolver_rejects_a_plan_that_does_not_authorize_execution() {
    let fixture = Fixture::new();
    let plan_path = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join("plan.json");
    let mut plan = fixture.plan.clone();
    plan.obligations[0].decision = ObligationDecision::ReusedFromTaskCache;
    fs::write(&plan_path, serde_json::to_vec(&plan).expect("plan JSON")).expect("replace plan");
    let error = fixture.resolve().expect_err("cached task cannot execute");
    assert_error(&error, "task_not_executable_in_plan");
}

#[test]
fn resolver_rejects_noncanonical_and_duplicate_manifest_data() {
    let fixture = Fixture::new();
    let path = fixture.repo.path().join(TASK_EXECUTION_MANIFEST_PATH);
    let body = serde_json::to_string(&fixture.manifest).expect("manifest JSON");
    fs::write(
        &path,
        format!(
            "{}\n{body}\n",
            fixture.manifest.marker_line().expect("marker")
        ),
    )
    .expect("write noncanonical but valid JSON");
    let error = fixture.resolve().expect_err("noncanonical JSON rejected");
    assert_error(&error, "noncanonical_task_execution_manifest");

    let marker = fixture.manifest.marker_line().expect("marker");
    fs::write(path, format!("{marker}\n{{\"schema\":1,\"schema\":1}}\n"))
        .expect("write duplicate-key JSON");
    let error = fixture.resolve().expect_err("duplicate key rejected");
    assert_error(&error, "unparsable_task_execution_manifest");
}

#[path = "task_execution_test_support.rs"]
mod support;
use support::*;
