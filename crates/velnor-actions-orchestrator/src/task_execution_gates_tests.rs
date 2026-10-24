//! Runtime task-gate regressions.

use super::{GENERATOR_VERSION, RUN_KEY, TASK_ID, fixture::Fixture, support};
use crate::task_execution::task_execution_gates;
use velnor_actions_contract::workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, NamedCheckLaneVariant, ObligationDecision,
    PlannedPlatform, TaskReport,
};
use velnor_actions_contract::{
    EdgeKind, TaskEdge, canonical_json_bytes, digest_b3, task_report_id_for_task,
};

const GITHUB_JOB: &str = "crate_clippy";
const PREREQUISITE: &str = "stack/rust/demo/fmt/default";
const INDEPENDENT: &str = "stack/rust/demo/test/default";

fn fixture_with_prerequisite() -> Fixture {
    let mut fixture = Fixture::new();
    let mut plan = fixture.plan.clone();
    let template = plan.matrix.include[0].clone();
    for (task_id, byte) in [(PREREQUISITE, 41), (INDEPENDENT, 42)] {
        let task_digest = support::digest(byte);
        let input_digest = support::digest(byte + 10);
        let mut obligation = plan.obligations[0].clone();
        obligation.task_id = task_id.to_owned();
        obligation.task_digest = task_digest.clone();
        obligation.input_digest = input_digest.clone();
        obligation.closure_digest = support::digest(byte + 20);
        obligation.decision = ObligationDecision::Execute;
        plan.obligations.push(obligation);
        let mut entry = MatrixEntry::derive(
            "rust",
            task_id,
            "true",
            &task_digest,
            serde_json::json!({}),
            ExecuteTaskIds {
                tasks: std::collections::BTreeMap::from([(
                    "task".to_owned(),
                    ExecuteTaskRef::Single(task_id.to_owned()),
                )]),
            },
            &input_digest,
            RUN_KEY,
            GITHUB_JOB,
            template.planned_platform.clone(),
        )
        .expect("prerequisite entry");
        entry.toolchain_inputs = template.toolchain_inputs.clone();
        entry.toolchain_id = template.toolchain_id.clone();
        plan.matrix.include.push(entry);
        plan.task_ids.push(task_id.to_owned());
    }
    plan.edges.push(TaskEdge {
        from: PREREQUISITE.to_owned(),
        to: TASK_ID.to_owned(),
        kind: EdgeKind::Gate,
    });
    plan.obligations
        .sort_by(|left, right| left.task_id.cmp(&right.task_id));
    plan.task_ids.sort();
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    support::write_plan(fixture.runner_temp.path(), &plan);
    fixture.plan = plan;
    fixture
}

fn entry<'a>(fixture: &'a Fixture, task_id: &str) -> &'a MatrixEntry {
    fixture
        .plan
        .matrix
        .include
        .iter()
        .find(|entry| task_id == entry.task_id)
        .expect("matrix entry")
}

fn prerequisite_report_path(fixture: &Fixture) -> std::path::PathBuf {
    let entry = entry(fixture, PREREQUISITE);
    let digest = fixture
        .plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == PREREQUISITE)
        .expect("obligation")
        .task_digest
        .as_str();
    let report_id = task_report_id_for_task(RUN_KEY, &entry.matrix_key, digest).expect("report ID");
    fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join(&entry.matrix_key)
        .join("tasks")
        .join(format!("{report_id}.json"))
}

fn write_prerequisite_report(fixture: &Fixture, exit_code: i32) -> TaskReport {
    let report = crate::task_report::terminal_task_report(
        &fixture.plan,
        entry(fixture, PREREQUISITE),
        &fixture
            .plan
            .obligations
            .iter()
            .find(|obligation| obligation.task_id == PREREQUISITE)
            .expect("obligation")
            .task_digest,
        exit_code,
        None,
    )
    .expect("task report");
    let path = prerequisite_report_path(fixture);
    std::fs::create_dir_all(path.parent().expect("report parent")).expect("report directory");
    std::fs::write(
        &path,
        canonical_json_bytes(&report).expect("canonical report"),
    )
    .expect("write report");
    report
}

#[test]
fn failed_prerequisite_blocks_dependent_but_not_independent_task() {
    let fixture = fixture_with_prerequisite();
    write_prerequisite_report(&fixture, 1);
    task_execution_gates::validate_prerequisites(
        &fixture.plan,
        entry(&fixture, INDEPENDENT),
        INDEPENDENT,
        GITHUB_JOB,
        fixture.runner_temp.path(),
    )
    .expect("independent task runs after another task fails");
    let error = task_execution_gates::validate_prerequisites(
        &fixture.plan,
        entry(&fixture, TASK_ID),
        TASK_ID,
        GITHUB_JOB,
        fixture.runner_temp.path(),
    )
    .expect_err("dependent task requires successful prerequisite");
    support::assert_error(&error, "task_prerequisite_report_not_successful");
}

#[test]
fn successful_prerequisite_allows_dependent_task() {
    let fixture = fixture_with_prerequisite();
    write_prerequisite_report(&fixture, 0);
    task_execution_gates::validate_prerequisites(
        &fixture.plan,
        entry(&fixture, TASK_ID),
        TASK_ID,
        GITHUB_JOB,
        fixture.runner_temp.path(),
    )
    .expect("successful prerequisite permits execution");
}

#[test]
fn missing_cross_job_report_fails_closed() {
    let fixture = fixture_with_prerequisite();
    let mut plan = fixture.plan.clone();
    let prerequisite = plan
        .matrix
        .include
        .iter_mut()
        .find(|entry| entry.task_id == PREREQUISITE)
        .expect("prerequisite entry");
    prerequisite.job_id = "rust-other".to_owned();
    prerequisite.artifact_id =
        velnor_actions_contract::artifact_id_for_crate_job(RUN_KEY, &prerequisite.job_id)
            .expect("cross-job artifact");
    let dependent = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id == TASK_ID)
        .expect("dependent entry");
    let error = task_execution_gates::validate_prerequisites(
        &plan,
        dependent,
        TASK_ID,
        GITHUB_JOB,
        fixture.runner_temp.path(),
    )
    .expect_err("missing upstream artifact cannot prove success");
    support::assert_error(&error, "task_prerequisite_report_missing_or_invalid");
}

#[test]
fn malformed_and_foreign_reports_fail_closed() {
    for mutation in ["malformed", "foreign", "digest", "matrix"] {
        let fixture = fixture_with_prerequisite();
        let mut report = write_prerequisite_report(&fixture, 0);
        match mutation {
            "malformed" => {
                std::fs::write(prerequisite_report_path(&fixture), b"{").expect("corrupt report");
            }
            "foreign" => {
                report.run_key = "r2-a1".to_owned();
                report.task_report_id =
                    task_report_id_for_task("r2-a1", &report.matrix_key, &report.task_digest)
                        .expect("foreign report identity");
                std::fs::write(
                    prerequisite_report_path(&fixture),
                    canonical_json_bytes(&report).expect("foreign report JSON"),
                )
                .expect("replace report");
            }
            "digest" => {
                report.task_digest = digest_b3(b"foreign digest");
                report.task_report_id =
                    task_report_id_for_task(RUN_KEY, &report.matrix_key, &report.task_digest)
                        .expect("foreign digest identity");
                std::fs::write(
                    prerequisite_report_path(&fixture),
                    canonical_json_bytes(&report).expect("digest report JSON"),
                )
                .expect("replace report");
            }
            "matrix" => {
                report.matrix_key = "m-0123456789abcdef".to_owned();
                report.matrix_id = "stack:rust|task:foreign".to_owned();
                report.task_report_id =
                    task_report_id_for_task(RUN_KEY, &report.matrix_key, &report.task_digest)
                        .expect("foreign matrix identity");
                std::fs::write(
                    prerequisite_report_path(&fixture),
                    canonical_json_bytes(&report).expect("matrix report JSON"),
                )
                .expect("replace report");
            }
            _ => unreachable!(),
        }
        let error = task_execution_gates::validate_prerequisites(
            &fixture.plan,
            entry(&fixture, TASK_ID),
            TASK_ID,
            GITHUB_JOB,
            fixture.runner_temp.path(),
        )
        .expect_err("untrusted prerequisite report rejected");
        assert!(error.to_string().contains("task_prerequisite"), "{error}");
    }
}

#[test]
fn lane_record_binds_to_the_selected_physical_job() {
    let fixture = Fixture::new();
    let record = fixture
        .manifest
        .tasks
        .get(TASK_ID)
        .expect("manifest record");
    let source = entry(&fixture, TASK_ID);
    assert_eq!(source.job_id, GITHUB_JOB);
    assert_eq!(source.task_id, record.task_id);
    assert!(
        source
            .execute_task_ids
            .tasks
            .values()
            .any(|reference| match reference {
                ExecuteTaskRef::Single(id) => id == TASK_ID,
                ExecuteTaskRef::Shards(ids) => ids.iter().any(|id| id == TASK_ID),
            })
    );
    assert_eq!(record.matrix_id, source.id);
    assert!(
        task_execution_gates::entry_matches_manifest(record, source, GITHUB_JOB,)
            .expect("normal job binding")
    );
    assert!(
        !task_execution_gates::entry_matches_manifest(record, source, "rust-another",)
            .expect("wrong job binding")
    );
}

#[test]
fn both_lanes_resolve_one_manifest_record_to_distinct_matrix_identities() {
    let fixture = Fixture::new();
    let record = fixture
        .manifest
        .tasks
        .get(TASK_ID)
        .expect("manifest record");
    let original = entry(&fixture, TASK_ID).clone();
    let mut plan = fixture.plan.clone();
    let (hosted, scale_set) = paired_lanes(original);
    plan.matrix.include = vec![hosted.clone(), scale_set.clone()];
    for obligation in &mut plan.obligations {
        obligation.job_id = "rust-demo__hosted".to_owned();
    }
    plan.jobs = vec!["rust-demo__hosted", "rust-demo__local"]
        .into_iter()
        .map(|job_id| velnor_actions_contract::PlanJob {
            job_id: job_id.to_owned(),
            disposition: velnor_actions_contract::JobDisposition::Run,
        })
        .collect();
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    support::write_plan(fixture.runner_temp.path(), &plan);

    for (job_id, expected) in [
        ("rust-demo__hosted", hosted),
        ("rust-demo__local", scale_set),
    ] {
        let frame = crate::task_execution::resolve_task_execution_to(
            fixture.repo.path(),
            fixture.runner_temp.path(),
            RUN_KEY,
            &record.execution_digest,
            GENERATOR_VERSION,
            job_id,
        )
        .expect("selected lane resolves task");
        let fields = support::nul_fields(&frame);
        assert_eq!(fields[4], expected.id);
        assert_eq!(fields[5], expected.matrix_key);
        assert!(
            task_execution_gates::entry_matches_manifest(record, &expected, job_id,)
                .expect("lane binds static manifest")
        );
    }
}

fn paired_lanes(original: MatrixEntry) -> (MatrixEntry, MatrixEntry) {
    let mut hosted = MatrixEntry::derive_for_lane(
        &original.stack_id,
        &original.task_id,
        &original.run,
        &original.task_digest,
        original.adapter_metadata.clone(),
        original.execute_task_ids.clone(),
        &original.input_digest,
        RUN_KEY,
        "rust-demo__hosted",
        Some(NamedCheckLaneVariant::Hosted),
        original.planned_platform.clone(),
    )
    .expect("hosted lane");
    hosted.toolchain_inputs = original.toolchain_inputs.clone();
    hosted.toolchain_id = original.toolchain_id.clone();
    let scale_set_platform = PlannedPlatform::new(
        "scale-set:velnor+orbstack-linux",
        &original.planned_platform.target,
    )
    .expect("Scale Set platform");
    let mut scale_set = MatrixEntry::derive_for_lane(
        &original.stack_id,
        &original.task_id,
        &original.run,
        &original.task_digest,
        original.adapter_metadata,
        original.execute_task_ids,
        &original.input_digest,
        RUN_KEY,
        "rust-demo__local",
        Some(NamedCheckLaneVariant::ScaleSet),
        scale_set_platform,
    )
    .expect("Scale Set lane");
    scale_set.toolchain_inputs = original.toolchain_inputs.clone();
    scale_set.toolchain_id = original.toolchain_id.clone();
    (hosted, scale_set)
}
