//! Task reports for sequential and parallel Rust task steps.

use tempfile::TempDir;
use velnor_actions_contract::{NotSelectedReason, Plan, TaskReport, TaskStatus};

use super::task_report_tests::{
    CLIPPY, TEST, entry_for, fixture_plan, obligation_for, read_entry, run_jobs, staged_run,
};
use super::*;

const JOB: &str = "crate_demo";
const DOCTEST: &str = "stack/rust/demo/doctest/default";
const DOC: &str = "stack/rust/demo/doc/default";
const BUILD: &str = "stack/rust/demo/build/default";
const OTHER_JOB_TASK: &str = "stack/rust/other/clippy/default";
const TOFU_FMT: &str = "stack/tofu/dir-737461636b732f61/fmt/default";
const TOFU_INIT: &str = "stack/tofu/dir-737461636b732f61/init/default";
const TOFU_VALIDATE: &str = "stack/tofu/dir-737461636b732f61/validate/default";

/// Valid fixture whose matrix order differs from its crate-job order.
pub(super) fn order_fixture(with_build: bool) -> Plan {
    let mut tasks = vec![
        (CLIPPY, "clippy", 1),
        (TEST, "test", 2),
        (DOCTEST, "doctest", 3),
        (DOC, "doc", 4),
    ];
    if with_build {
        tasks.push((BUILD, "build", 5));
    }
    let mut entries = Vec::new();
    let mut obligations = Vec::new();
    for (task_id, kind, seed) in tasks {
        let (entry, task_digest) = entry_for("rust", task_id, kind, seed, JOB);
        entries.push(entry);
        obligations.push(obligation_for(task_id, task_digest, seed, JOB));
    }
    let mut plan = fixture_plan();
    plan.matrix.include = entries;
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    plan.obligations = obligations;
    plan.obligations
        .sort_by(|left, right| left.task_id.cmp(&right.task_id));
    plan.task_ids = plan
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    plan.jobs = run_jobs(&plan);
    plan.validate().expect("order fixture validates");
    plan
}

/// Add one valid task that belongs to a different job.
fn with_other_job(mut plan: Plan) -> Plan {
    let (entry, digest) = entry_for("rust", OTHER_JOB_TASK, "clippy", 20, "crate_other");
    plan.matrix.include.push(entry);
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    plan.obligations
        .push(obligation_for(OTHER_JOB_TASK, digest, 20, "crate_other"));
    plan.obligations
        .sort_by(|left, right| left.task_id.cmp(&right.task_id));
    plan.task_ids.push(OTHER_JOB_TASK.to_owned());
    plan.task_ids.sort();
    plan.jobs = run_jobs(&plan);
    plan.validate().expect("cross-job fixture validates");
    plan
}

/// Valid `ToFu` job with its three sequential task kinds.
fn tofu_order_fixture() -> Plan {
    let kinds = [
        (velnor_actions_tofu::TofuTaskKind::Fmt, 1),
        (velnor_actions_tofu::TofuTaskKind::InitForValidate, 2),
        (velnor_actions_tofu::TofuTaskKind::Validate, 3),
    ];
    let mut entries = Vec::new();
    let mut obligations = Vec::new();
    for (kind, seed) in kinds {
        let task_id = velnor_actions_tofu::task_id_for_root("stacks/a", kind, "default")
            .expect("ToFu task ID");
        let (entry, digest) = entry_for("tofu", &task_id, kind.as_str(), seed, "tofu_a");
        entries.push(entry);
        obligations.push(obligation_for(&task_id, digest, seed, "tofu_a"));
    }
    let mut plan = fixture_plan();
    plan.matrix.include = entries;
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    plan.obligations = obligations;
    plan.obligations
        .sort_by(|left, right| left.task_id.cmp(&right.task_id));
    plan.task_ids = plan
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    plan.jobs = run_jobs(&plan);
    plan.validate().expect("ToFu order fixture validates");
    plan
}

/// Read the task report associated with one task in a plan.
fn task_report(plan: &Plan, temp: &TempDir, task_id: &str) -> TaskReport {
    let entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| {
            entry
                .execute_task_ids
                .tasks
                .values()
                .any(|task_ref| match task_ref {
                    velnor_actions_contract::ExecuteTaskRef::Single(id) => id == task_id,
                    velnor_actions_contract::ExecuteTaskRef::Shards(ids) => {
                        ids.iter().any(|id| id == task_id)
                    }
                })
        })
        .expect("task entry");
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .expect("task obligation")
        .task_digest
        .as_str();
    let report_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, digest)
            .expect("report ID");
    read_entry(temp, "local", &entry.matrix_key, &report_id).0
}

/// Path for one plan-bound task report.
fn task_report_path(plan: &Plan, temp: &TempDir, task_id: &str) -> std::path::PathBuf {
    let entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| super::flattened(entry).contains(&task_id))
        .expect("task entry");
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .expect("task obligation")
        .task_digest
        .as_str();
    let report_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, digest)
            .expect("report ID");
    temp.path()
        .join("velnor/local")
        .join(&entry.matrix_key)
        .join("tasks")
        .join(format!("{report_id}.json"))
}

#[test]
fn parallel_member_failure_does_not_block_sibling_reports() {
    let plan = order_fixture(false);
    for task_id in [CLIPPY, TEST, DOCTEST, DOC] {
        assert_eq!(
            super::task_report_order::derive_downstream(&plan, task_id, JOB),
            Vec::<String>::new(),
            "parallel task cannot synthesize sibling skips: {task_id}"
        );
    }
    let failed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_to("local", CLIPPY, 1, None, failed.path())
            .expect("a failed member writes only its own report"),
        1
    );
    for task_id in [TEST, DOCTEST, DOC] {
        assert!(
            !task_report_path(&plan, &failed, task_id).exists(),
            "failed parallel member does not write a sibling report: {task_id}"
        );
    }
    for task_id in [TEST, DOCTEST, DOC] {
        assert_eq!(
            write_task_report_to("local", task_id, 0, None, failed.path())
                .expect("siblings still write their own reports"),
            1
        );
        assert_eq!(
            task_report(&plan, &failed, task_id).status,
            TaskStatus::Executed,
            "{task_id}"
        );
    }
}

#[test]
fn failed_build_reports_unstarted_checks_as_upstream_failed() {
    let plan = order_fixture(true);
    assert_eq!(
        super::task_report_order::derive_downstream(&plan, BUILD, JOB),
        [CLIPPY, TEST, DOCTEST, DOC].map(str::to_owned),
        "sequential build failure skips the later parallel group in task order"
    );
    let failed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_to("local", BUILD, 1, None, failed.path())
            .expect("build failure reports the skipped group"),
        5
    );
    assert_eq!(
        task_report(&plan, &failed, BUILD).status,
        TaskStatus::Failed
    );
    for task_id in [CLIPPY, TEST, DOCTEST, DOC] {
        let task = task_report(&plan, &failed, task_id);
        assert_eq!(task.status, TaskStatus::NotSelected, "{task_id}");
        assert_eq!(
            task.not_selected_reason,
            Some(NotSelectedReason::UpstreamFailed),
            "{task_id}"
        );
    }
}

#[test]
fn successful_parallel_member_does_not_synthesize_sibling_reports() {
    let plan = order_fixture(false);
    for task_id in [CLIPPY, TEST, DOCTEST, DOC] {
        assert_eq!(
            super::task_report_order::derive_downstream(&plan, task_id, JOB),
            Vec::<String>::new(),
            "parallel task cannot synthesize sibling skips: {task_id}"
        );
    }
    let passed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_to("local", CLIPPY, 0, None, passed.path())
            .expect("successful parallel task writes only itself"),
        1
    );
    for task_id in [TEST, DOCTEST, DOC] {
        assert!(
            !task_report_path(&plan, &passed, task_id).exists(),
            "success does not synthesize a sibling report: {task_id}"
        );
    }
}

#[test]
fn explicit_downstream_selection_is_a_normalized_safe_subset() {
    let plan = order_fixture(true);
    let selected = staged_run(&plan, "local");
    let raw = format!(" , {TEST}, {TEST} , ");
    assert_eq!(
        write_task_report_for_job_with_downstream(
            "local",
            BUILD,
            1,
            None,
            JOB,
            Some(&raw),
            selected.path(),
        )
        .expect("valid downstream subset"),
        2
    );
    assert_eq!(
        task_report(&plan, &selected, TEST).not_selected_reason,
        Some(NotSelectedReason::UpstreamFailed)
    );
    assert!(!task_report_path(&plan, &selected, CLIPPY).exists());

    let empty = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_for_job_with_downstream(
            "local",
            BUILD,
            1,
            None,
            JOB,
            Some(" , \t, "),
            empty.path(),
        )
        .expect("normalized-empty selection derives all skips"),
        5
    );
}

#[test]
fn invalid_downstream_ids_fail_before_any_skip_report_is_written() {
    let plan = with_other_job(order_fixture(true));
    for raw in [BUILD, "stack/rust/unknown/default"] {
        let temp = staged_run(&plan, "local");
        assert!(
            write_task_report_for_job_with_downstream(
                "local",
                BUILD,
                1,
                None,
                JOB,
                Some(raw),
                temp.path(),
            )
            .is_err()
        );
        assert!(task_report_path(&plan, &temp, BUILD).exists());
        assert!(!task_report_path(&plan, &temp, TEST).exists());
    }

    let temp = staged_run(&plan, "local");
    let raw = format!("{TEST}, {OTHER_JOB_TASK}");
    assert!(
        write_task_report_for_job_with_downstream(
            "local",
            BUILD,
            1,
            None,
            JOB,
            Some(&raw),
            temp.path(),
        )
        .is_err()
    );
    assert!(task_report_path(&plan, &temp, BUILD).exists());
    assert!(
        !task_report_path(&plan, &temp, TEST).exists(),
        "a valid earlier selection is not written before a later cross-job rejection"
    );

    let parallel = order_fixture(false);
    let temp = staged_run(&parallel, "local");
    assert!(
        write_task_report_for_job_with_downstream(
            "local",
            CLIPPY,
            1,
            None,
            JOB,
            Some(TEST),
            temp.path(),
        )
        .is_err()
    );
    assert!(task_report_path(&parallel, &temp, CLIPPY).exists());
    assert!(!task_report_path(&parallel, &temp, TEST).exists());
}

#[test]
fn failed_tofu_task_reports_only_later_sequential_obligations() {
    let plan = tofu_order_fixture();
    let ordered = super::task_report_order::derive_downstream(&plan, TOFU_FMT, "tofu_a");
    assert_eq!(ordered, [TOFU_INIT, TOFU_VALIDATE].map(str::to_owned));
    let failed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_for_job("local", TOFU_FMT, 1, None, "tofu_a", failed.path())
            .expect("failed ToFu task reports later obligations"),
        3
    );
    for task_id in ordered {
        let task = task_report(&plan, &failed, &task_id);
        assert_eq!(task.status, TaskStatus::NotSelected, "{task_id}");
        assert_eq!(
            task.not_selected_reason,
            Some(NotSelectedReason::UpstreamFailed)
        );
    }
}

#[test]
fn failed_tofu_init_skips_validate_without_rewriting_prior_fmt() {
    let plan = tofu_order_fixture();
    let failed = staged_run(&plan, "local");
    assert_eq!(
        super::task_report_order::derive_downstream(&plan, TOFU_INIT, "tofu_a"),
        [TOFU_VALIDATE.to_owned()]
    );
    assert_eq!(
        write_task_report_for_job("local", TOFU_INIT, 1, None, "tofu_a", failed.path())
            .expect("failed ToFu init skips later validate"),
        2
    );
    assert!(!task_report_path(&plan, &failed, TOFU_FMT).exists());
    let validate = task_report(&plan, &failed, TOFU_VALIDATE);
    assert_eq!(validate.status, TaskStatus::NotSelected);
    assert_eq!(
        validate.not_selected_reason,
        Some(NotSelectedReason::UpstreamFailed)
    );
}
