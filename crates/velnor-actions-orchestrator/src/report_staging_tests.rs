//! Report-payload staging tests: closed plan inventory and byte preservation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, Plan, matrix_json_bytes,
};

use super::*;

use crate::task_report::task_report_tests::fixture_plan;

const CLIPPY: &str = "stack/rust/demo/clippy/default";
const TEST: &str = "stack/rust/demo/test/default";
const ACTION_TASK: &str = "stack/workload/container/build/docker_build";

fn run_dir(temp: &TempDir) -> PathBuf {
    temp.path().join("velnor").join("local")
}

fn source_path(temp: &TempDir, relative: &Path) -> PathBuf {
    run_dir(temp).join(relative)
}

fn destination_path(temp: &TempDir, relative: &Path) -> PathBuf {
    temp.path()
        .join("velnor")
        .join("report-payload")
        .join("local")
        .join(relative)
}

fn staged_plan(plan: &Plan) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let dir = temp.path().join("velnor").join(&plan.run_key);
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(
        dir.join("plan.json"),
        serde_json::to_vec(plan).expect("plan bytes"),
    )
    .expect("plan");
    fs::write(
        dir.join("matrix.json"),
        matrix_json_bytes(&plan.matrix).expect("matrix bytes"),
    )
    .expect("matrix");
    temp
}

fn write_source_file(temp: &TempDir, relative: &Path, bytes: &[u8]) {
    let path = source_path(temp, relative);
    fs::create_dir_all(path.parent().expect("source parent")).expect("source dirs");
    fs::write(path, bytes).expect("source bytes");
}

fn task_relative(plan: &Plan, entry_index: usize, task_id: &str) -> PathBuf {
    let entry = &plan.matrix.include[entry_index];
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .map(|obligation| obligation.task_digest.as_str())
        .expect("task obligation");
    let report =
        velnor_actions_contract::task_report_id_for_task(&plan.run_key, &entry.matrix_key, digest)
            .expect("task report ID");
    PathBuf::from(&entry.matrix_key)
        .join("tasks")
        .join(format!("{report}.json"))
}

fn all_expected_bytes(plan: &Plan) -> BTreeMap<PathBuf, Vec<u8>> {
    expected_paths(plan)
        .expect("expected paths")
        .keys()
        .map(|path| {
            (
                path.clone(),
                format!("payload:{}", path.display()).into_bytes(),
            )
        })
        .collect()
}

fn action_plan() -> Plan {
    let mut plan = fixture_plan();
    let old = plan.obligations[0].clone();
    let mut obligation = old;
    obligation.task_id = ACTION_TASK.to_owned();
    obligation.job_id = "workload-container".to_owned();
    let action_digest = obligation.task_digest.clone();
    let action_input = obligation.input_digest.clone();
    let mut entry = MatrixEntry::derive(
        "workload",
        ACTION_TASK,
        "true",
        &action_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "build".to_owned(),
                ExecuteTaskRef::Single(ACTION_TASK.to_owned()),
            )]),
        },
        &action_input,
        &plan.run_key,
        &obligation.job_id,
    )
    .expect("action entry");
    let action_id = format!("velnor-action-{}", entry.matrix_key);
    entry.adapter_metadata = serde_json::json!({
        "configuration": "docker_build",
        "kind": "build",
        "action": {
            "id": action_id,
            "uses": format!(
                "docker/build-push-action@{}",
                velnor_actions_actionlint::actions::BUILD_PUSH_ACTION_SHA
            ),
        },
    });
    plan.obligations = vec![obligation];
    plan.matrix.include = vec![entry];
    plan.task_ids = vec![ACTION_TASK.to_owned()];
    plan.validate().expect("action plan");
    plan
}

#[test]
fn authority_inventory_excludes_plan_and_noise_and_preserves_bytes() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let expected = all_expected_bytes(&plan);
    for (relative, bytes) in &expected {
        write_source_file(&temp, relative, bytes);
    }
    write_source_file(&temp, Path::new("baseline.json"), b"baseline-noise");
    write_source_file(&temp, Path::new("final-report.json"), b"final-noise");
    write_source_file(&temp, Path::new("helper"), b"helper-noise");
    write_source_file(&temp, Path::new("start-clippy"), b"start-noise");
    write_source_file(
        &temp,
        &PathBuf::from(&plan.matrix.include[0].matrix_key)
            .join("tasks")
            .join("stray.json"),
        b"stray-task-noise",
    );
    write_source_file(
        &temp,
        &PathBuf::from(&plan.matrix.include[0].matrix_key).join("extra.json"),
        b"stray-entry-noise",
    );

    let count = stage_reports_to("local", temp.path()).expect("stage reports");
    assert_eq!(count, expected.len());
    for (relative, bytes) in expected {
        assert_eq!(
            fs::read(destination_path(&temp, &relative)).expect("staged bytes"),
            bytes
        );
    }
    for relative in [
        Path::new("plan.json"),
        Path::new("matrix.json"),
        Path::new("baseline.json"),
        Path::new("final-report.json"),
        Path::new("helper"),
        Path::new("start-clippy"),
    ] {
        assert!(
            !destination_path(&temp, relative).exists(),
            "noise: {relative:?}"
        );
    }
    assert!(
        !destination_path(
            &temp,
            &PathBuf::from(&plan.matrix.include[0].matrix_key)
                .join("tasks")
                .join("stray.json")
        )
        .exists()
    );
    assert!(
        !destination_path(
            &temp,
            &PathBuf::from(&plan.matrix.include[0].matrix_key).join("extra.json")
        )
        .exists()
    );
}

#[test]
fn failed_and_partial_bytes_survive_without_matrix_report() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let failed_matrix =
        PathBuf::from(&plan.matrix.include[0].matrix_key).join("matrix-report.json");
    write_source_file(&temp, &failed_matrix, b"failed-matrix-evidence");
    let failed_task = task_relative(&plan, 0, CLIPPY);
    write_source_file(&temp, &failed_task, b"failed-task-evidence");
    let partial_task = task_relative(&plan, 1, TEST);
    write_source_file(&temp, &partial_task, b"partial-task-evidence");

    let count = stage_reports_to("local", temp.path()).expect("stage partial evidence");
    assert_eq!(count, 3);
    assert_eq!(
        fs::read(destination_path(&temp, &failed_matrix)).expect("failed matrix"),
        b"failed-matrix-evidence"
    );
    assert_eq!(
        fs::read(destination_path(&temp, &failed_task)).expect("failed task"),
        b"failed-task-evidence"
    );
    assert_eq!(
        fs::read(destination_path(&temp, &partial_task)).expect("partial task"),
        b"partial-task-evidence"
    );
    assert!(
        !destination_path(
            &temp,
            &PathBuf::from(&plan.matrix.include[1].matrix_key).join("matrix-report.json")
        )
        .exists()
    );
}

#[test]
fn missing_expected_files_are_omitted_without_error() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let matrix = PathBuf::from(&plan.matrix.include[0].matrix_key).join("matrix-report.json");
    write_source_file(&temp, &matrix, b"only-existing-evidence");

    assert_eq!(
        stage_reports_to("local", temp.path()).expect("stage sparse payload"),
        1
    );
    assert_eq!(
        fs::read(destination_path(&temp, &matrix)).expect("matrix"),
        b"only-existing-evidence"
    );
    let expected = expected_paths(&plan).expect("expected paths");
    assert!(
        expected
            .keys()
            .filter(|path| *path != &matrix)
            .all(|path| !destination_path(&temp, path).exists())
    );
}

#[test]
fn shard_references_derive_one_task_path_per_digest() {
    let mut plan = fixture_plan();
    plan.matrix.include.truncate(1);
    plan.obligations[1].job_id = plan.obligations[0].job_id.clone();
    plan.matrix.include[0].execute_task_ids.tasks = BTreeMap::from([(
        "shards".to_owned(),
        ExecuteTaskRef::Shards(vec![CLIPPY.to_owned(), TEST.to_owned()]),
    )]);
    plan.validate().expect("sharded plan");

    let paths = expected_paths(&plan).expect("shard paths");
    assert_eq!(paths.len(), 3);
    assert!(paths.contains_key(
        &PathBuf::from(&plan.matrix.include[0].matrix_key).join("matrix-report.json")
    ));
    for task_id in [CLIPPY, TEST] {
        assert!(
            paths.contains_key(&task_relative(&plan, 0, task_id)),
            "missing shard {task_id}"
        );
    }
}

#[test]
fn actionable_partial_payload_includes_action_bytes_without_matrix() {
    let plan = action_plan();
    let temp = staged_plan(&plan);
    let home = PathBuf::from(&plan.matrix.include[0].matrix_key);
    let begin = home.join("actions/begin.json");
    let report = home.join("actions/report.json");
    write_source_file(&temp, &begin, b"begin-evidence");
    write_source_file(&temp, &report, b"report-evidence");

    assert_eq!(
        stage_reports_to("local", temp.path()).expect("stage action evidence"),
        2
    );
    assert_eq!(
        fs::read(destination_path(&temp, &begin)).expect("begin"),
        b"begin-evidence"
    );
    assert_eq!(
        fs::read(destination_path(&temp, &report)).expect("report"),
        b"report-evidence"
    );
    assert!(!destination_path(&temp, &home.join("matrix-report.json")).exists());
}

#[test]
fn seeded_destination_root_is_refused_before_copying() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let destination = temp.path().join("velnor/report-payload/local");
    fs::create_dir_all(&destination).expect("seed destination");
    fs::write(destination.join("sentinel"), b"keep").expect("sentinel");

    let error = stage_reports_to("local", temp.path()).expect_err("seed refused");
    assert!(
        error
            .to_string()
            .contains("report_payload_exists_or_unwritable")
    );
    assert_eq!(
        fs::read(destination.join("sentinel")).expect("sentinel"),
        b"keep"
    );
}

#[cfg(unix)]
#[test]
fn source_symlink_is_refused_without_following_it() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let target = temp.path().join("outside-plan.json");
    fs::write(&target, b"outside").expect("target");
    let source = run_dir(&temp);
    fs::remove_file(source.join("plan.json")).expect("plan");
    std::os::unix::fs::symlink(&target, source.join("plan.json")).expect("plan symlink");

    let error = stage_reports_to("local", temp.path()).expect_err("source symlink refused");
    assert!(error.to_string().contains("symlink_refused"));
    assert!(!temp.path().join("velnor/report-payload").exists());
}

#[cfg(unix)]
#[test]
fn source_matrix_ancestor_symlink_is_refused() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let outside = TempDir::new().expect("outside temp");
    fs::write(outside.path().join("matrix-report.json"), b"outside").expect("outside report");
    let entry_home = run_dir(&temp).join(&plan.matrix.include[0].matrix_key);
    std::os::unix::fs::symlink(outside.path(), &entry_home).expect("entry symlink");

    let error = stage_reports_to("local", temp.path()).expect_err("entry symlink refused");
    assert!(error.to_string().contains("symlink_refused"));
    assert!(!temp.path().join("velnor/report-payload").exists());
}

#[cfg(unix)]
#[test]
fn destination_symlink_is_refused_without_following_it() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let outside = TempDir::new().expect("outside temp");
    let payload = temp.path().join("velnor/report-payload");
    std::os::unix::fs::symlink(outside.path(), &payload).expect("payload symlink");

    let error = stage_reports_to("local", temp.path()).expect_err("destination symlink refused");
    assert!(error.to_string().contains("symlink_refused"));
    assert!(!outside.path().join("local").exists());
}

#[test]
fn oversized_expected_report_is_rejected_before_destination_creation() {
    let plan = fixture_plan();
    let temp = staged_plan(&plan);
    let matrix = PathBuf::from(&plan.matrix.include[0].matrix_key).join("matrix-report.json");
    write_source_file(&temp, &matrix, &vec![b'x'; REPORT_BOUND as usize + 1]);

    let error = stage_reports_to("local", temp.path()).expect_err("oversize refused");
    assert!(error.to_string().contains("report_payload_oversize"));
    assert!(!temp.path().join("velnor/report-payload").exists());
}

#[cfg(unix)]
#[path = "report_staging_fifo_tests.rs"]
mod report_staging_fifo_tests;
