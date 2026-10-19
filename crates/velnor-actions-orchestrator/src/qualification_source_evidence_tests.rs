//! Source evidence file checks and exact report bindings.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanGenerator,
    PlanMatrix, PlanObligation, PlanRunner, QualificationCacheProducerContext,
    QualificationDispatch, QualificationDispatchMode, QualificationPhase, QualificationRunRef,
    TaskStatus, Trust, WorkflowEvent,
};
use velnor_actions_contract::{
    PlannedPlatform, RunnerSelection, canonical_json_bytes, plan_id_for_run,
};

use super::reports::read_successful_lane_reports;
use super::types::QualificationSourceEvidenceDocument;
use super::{SOURCE_EVIDENCE_FILENAME, validate_runner_directories};
use crate::qualification_cache_runtime::closure::ObservedLaneClosure;
use crate::task_report::{single_task_aggregate, terminal_task_report, write_entry_reports};

#[path = "qualification_source_evidence_group_tests.rs"]
mod group_tests;
#[path = "qualification_source_evidence_mode_tests.rs"]
mod mode_tests;

const RUN_ID: u64 = 720;
const RUN_KEY: &str = "r720-a1";
const TASK_ID: &str = "stack/rust/demo/clippy/default";
const JOB_ID: &str = "qualification-rust-demo";

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

fn entry_and_obligation() -> (MatrixEntry, PlanObligation) {
    let task_digest = digest(1);
    let input_digest = digest(2);
    let closure_digest = digest(3);
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
        &input_digest,
        RUN_KEY,
        JOB_ID,
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform"),
    )
    .expect("entry");
    let obligation = PlanObligation {
        task_id: TASK_ID.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest: task_digest.clone(),
        task_binding_digest: velnor_actions_contract::TaskBindingDigest::new(task_digest)
            .expect("binding digest"),
        input_digest,
        closure_digest,
        graph_digest: "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        mbx_digest: "b3-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        profile: "default".to_owned(),
        job_id: JOB_ID.to_owned(),
    };
    (entry, obligation)
}

fn fixture() -> (TempDir, Plan, MatrixEntry, PlanObligation) {
    let (entry, obligation) = entry_and_obligation();
    let source = "0123456789abcdef0123456789abcdef01234567".to_owned();
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: RUN_KEY.to_owned(),
        plan_id: plan_id_for_run(RUN_KEY).expect("plan ID"),
        base: None,
        head: source.clone(),
        event: WorkflowEvent::Qualification,
        qualification: Some(QualificationDispatch {
            mode: QualificationDispatchMode::Controlled,
            campaign: "source-test".to_owned(),
            phase: QualificationPhase::Cold,
            repository: "owner/repo".to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
            workflow_sha: source.clone(),
            source_sha: source,
            run_id: RUN_ID,
            run_attempt: 1,
            predecessor: None,
            scale_set_image_source_sha: None,
        }),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![obligation.clone()],
        matrix: PlanMatrix {
            include: vec![entry.clone()],
        },
        task_ids: vec![TASK_ID.to_owned()],
        jobs: vec![velnor_actions_contract::PlanJob {
            job_id: JOB_ID.to_owned(),
            disposition: velnor_actions_contract::JobDisposition::Run,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("plan validates");
    (
        TempDir::new().expect("runner temp"),
        plan,
        entry,
        obligation,
    )
}

fn write_reports(temp: &TempDir, plan: &Plan, entry: &MatrixEntry, exit_code: i32) {
    let task = terminal_task_report(plan, entry, &entry.task_digest, exit_code, Some(1))
        .expect("task report");
    let matrix = single_task_aggregate(plan, entry, &task).expect("matrix report");
    write_entry_reports(temp.path(), plan, entry, &task, &matrix).expect("write reports");
}

#[test]
fn successful_task_and_matrix_reports_are_bound_to_the_plan_lane() {
    let (temp, plan, entry, _) = fixture();
    write_reports(&temp, &plan, &entry, 0);

    let reports = read_successful_lane_reports(temp.path(), &plan, &entry, &entry.task_digest)
        .expect("successful reports");
    assert_eq!(reports.task.status, TaskStatus::Executed);
    assert_eq!(reports.task.exit_code, 0);
    assert_eq!(reports.matrix.report_id, entry.report_id);
    assert_eq!(reports.task.digest_b3.len(), 67);
    assert_eq!(reports.matrix.digest_b3.len(), 67);
}

#[test]
fn failed_task_report_cannot_produce_source_evidence() {
    let (temp, plan, entry, _) = fixture();
    write_reports(&temp, &plan, &entry, 1);

    let error = read_successful_lane_reports(temp.path(), &plan, &entry, &entry.task_digest)
        .expect_err("failed task is not evidence");
    assert!(
        error
            .to_string()
            .contains("qualification_source_task_report_mismatch")
    );
}

#[test]
fn noncanonical_or_mismatched_matrix_report_is_rejected() {
    let (temp, plan, entry, _) = fixture();
    write_reports(&temp, &plan, &entry, 0);
    let matrix_path = temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join(&entry.matrix_key)
        .join("matrix-report.json");
    let bytes = fs::read(&matrix_path).expect("matrix report");
    let mut noncanonical = vec![b' '];
    noncanonical.extend(bytes);
    fs::write(&matrix_path, noncanonical).expect("add extra report prefix");

    let error = read_successful_lane_reports(temp.path(), &plan, &entry, &entry.task_digest)
        .expect_err("report with extra prefix is rejected");
    assert!(
        error
            .to_string()
            .contains("qualification_source_matrix_report_not_canonical")
    );
}

#[test]
fn source_sidecar_records_observations_but_never_claims_cache_completion() {
    let (temp, plan, entry, obligation) = fixture();
    write_reports(&temp, &plan, &entry, 0);
    let reports = read_successful_lane_reports(temp.path(), &plan, &entry, &entry.task_digest)
        .expect("reports");
    let producer = QualificationCacheProducerContext {
        mode: QualificationDispatchMode::Controlled,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: plan.head.clone(),
        source_sha: plan.head.clone(),
        run: QualificationRunRef {
            run_id: RUN_ID,
            run_attempt: 1,
        },
    };
    let observed = ObservedLaneClosure {
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        observed_task_digest: entry.task_digest.clone(),
        observed_input_digest: entry.input_digest.clone(),
        observed_closure_digest: obligation.closure_digest.clone(),
    };
    let authenticated = super::context::AuthenticatedProducerContext {
        dispatch: plan.qualification.clone().expect("dispatch"),
        producer,
        runner_temp: temp.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        run_key: RUN_KEY.to_owned(),
    };

    super::write_sidecar(
        temp.path(),
        &plan,
        &entry,
        &authenticated,
        &plan.generator,
        &observed,
        reports,
    )
    .expect("write source evidence file");

    let path = temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join(&entry.matrix_key)
        .join(SOURCE_EVIDENCE_FILENAME);
    let bytes = fs::read(path).expect("source evidence file");
    let value: QualificationSourceEvidenceDocument =
        serde_json::from_slice(&bytes).expect("source evidence file JSON");
    assert_eq!(value.evidence.source_sha, plan.head);
    assert_eq!(
        value.evidence.closure.observed_input_digest,
        entry.input_digest
    );
    assert!(!value.evidence.cache_evidence_complete);
    assert_eq!(
        canonical_json_bytes(&value).expect("required source evidence file JSON"),
        bytes
    );
}

#[test]
fn source_sidecar_refuses_a_second_write() {
    let (temp, plan, entry, obligation) = fixture();
    write_reports(&temp, &plan, &entry, 0);
    let reports = read_successful_lane_reports(temp.path(), &plan, &entry, &entry.task_digest)
        .expect("reports");
    let producer = QualificationCacheProducerContext {
        mode: QualificationDispatchMode::Controlled,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: plan.head.clone(),
        source_sha: plan.head.clone(),
        run: QualificationRunRef {
            run_id: RUN_ID,
            run_attempt: 1,
        },
    };
    let authenticated = super::context::AuthenticatedProducerContext {
        dispatch: plan.qualification.clone().expect("dispatch"),
        producer,
        runner_temp: temp.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        run_key: RUN_KEY.to_owned(),
    };
    let observed = ObservedLaneClosure {
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        observed_task_digest: entry.task_digest.clone(),
        observed_input_digest: entry.input_digest.clone(),
        observed_closure_digest: obligation.closure_digest,
    };
    super::write_sidecar(
        temp.path(),
        &plan,
        &entry,
        &authenticated,
        &plan.generator,
        &observed,
        reports,
    )
    .expect("first source evidence file write");
    let error = super::write_sidecar(
        temp.path(),
        &plan,
        &entry,
        &authenticated,
        &plan.generator,
        &observed,
        super::reports::read_successful_lane_reports(
            temp.path(),
            &plan,
            &entry,
            &entry.task_digest,
        )
        .expect("reports remain available"),
    )
    .expect_err("source evidence file allows one write");
    assert!(
        error
            .to_string()
            .contains("qualification_source_evidence_exists")
    );
}

#[test]
fn staged_tree_directory_links_are_rejected() {
    let (temp, _, _, _) = fixture();
    fs::create_dir(temp.path().join("velnor")).expect("parent");
    #[cfg(unix)]
    std::os::unix::fs::symlink(temp.path(), temp.path().join("velnor/r720-a1"))
        .expect("run symlink");
    #[cfg(unix)]
    let error =
        validate_runner_directories(temp.path(), RUN_KEY, None).expect_err("run symlink rejected");
    #[cfg(unix)]
    assert!(
        error
            .to_string()
            .contains("qualification_source_directory_untrusted")
    );
}

#[cfg(unix)]
#[test]
fn matrix_and_task_directories_cannot_redirect_report_reads() {
    use std::os::unix::fs::symlink;

    let (matrix_temp, _, entry, _) = fixture();
    let run_dir = matrix_temp.path().join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    symlink(matrix_temp.path(), run_dir.join(&entry.matrix_key)).expect("matrix symlink");
    let error = validate_runner_directories(matrix_temp.path(), RUN_KEY, Some(&entry.matrix_key))
        .expect_err("matrix symlink is rejected");
    assert!(
        error
            .to_string()
            .contains("qualification_source_directory_untrusted")
    );

    let (task_temp, _, entry, _) = fixture();
    let matrix_dir = task_temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join(&entry.matrix_key);
    fs::create_dir_all(&matrix_dir).expect("matrix directory");
    symlink(task_temp.path(), matrix_dir.join("tasks")).expect("task symlink");
    let error = validate_runner_directories(task_temp.path(), RUN_KEY, Some(&entry.matrix_key))
        .expect_err("task symlink is rejected");
    assert!(
        error
            .to_string()
            .contains("qualification_source_directory_untrusted")
    );
}
