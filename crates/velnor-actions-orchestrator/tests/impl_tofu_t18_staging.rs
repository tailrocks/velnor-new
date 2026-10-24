//! Root lane staging and rendered cap cases.

use super::*;

/// Staging needs chain lanes: at most `max_parallel_jobs` root jobs
/// run concurrently (job[i] waits for job[i - max]).
#[test]
fn root_jobs_stage_needs_by_max_parallel() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    assert_eq!(
        jobs[&ids[0]].needs,
        vec![PLAN_JOB_ID.to_owned()],
        "first lane needs plan only"
    );
    assert_eq!(
        jobs[&ids[1]].needs,
        vec![PLAN_JOB_ID.to_owned()],
        "second lane needs plan only"
    );
    assert_eq!(
        jobs[&ids[2]].needs,
        vec![PLAN_JOB_ID.to_owned(), ids[0].clone()],
        "third job waits for the first lane"
    );
    Ok(())
}

/// Rendered root jobs declare the cap as `max-parallel` while the
/// marker trio itself never reaches the YAML.
#[test]
fn root_jobs_render_capped_strategy_without_markers() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let prep = prepare(dir.path())?;
    let jobs = finalized_jobs(&prep)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    for id in &ids {
        let window = job_window(yaml, id)?;
        assert!(
            window.contains("max-parallel: 2"),
            "{id} declares the configured cap:\n{window}"
        );
    }
    for marker in [
        "VELNOR_MATRIX_NEEDS_JOB",
        "VELNOR_MATRIX_OUTPUT",
        "VELNOR_MATRIX_MAX_PARALLEL",
    ] {
        assert!(
            !yaml.contains(marker),
            "marker {marker} never renders:\n{yaml}"
        );
    }
    Ok(())
}
