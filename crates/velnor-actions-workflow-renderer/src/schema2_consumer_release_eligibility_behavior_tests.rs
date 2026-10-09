use std::error::Error;

use super::support::{
    DEFAULT_BRANCH, OTHER_SHA, SOURCE_TOKEN, Scenario, assert_rejected, ci_run, execute, job_pages,
    pages, required_job,
};

#[test]
fn accepts_paginated_exact_source_push_ci_with_one_successful_required_job()
-> Result<(), Box<dyn Error>> {
    let scenario = Scenario::valid();
    let result = execute(&scenario)?;
    assert!(result.success, "{}", result.stderr);
    let source_sha = result
        .output
        .lines()
        .find_map(|line| line.strip_prefix("source_sha="))
        .expect("eligible output includes source SHA");
    assert_eq!(
        result.output,
        format!(
            "source_sha={source_sha}\nworkflow_authority_sha={source_sha}\nci_run_id=42\nci_attempt=3\n"
        )
    );
    assert!(result.calls.contains("actions/workflows/ci.yml/runs"));
    assert!(
        result
            .calls
            .contains("actions/runs/42/attempts/3/jobs?per_page=100")
    );
    assert_eq!(
        result
            .calls
            .lines()
            .filter(|line| line.contains("actions/runs/") && line.contains("/jobs?"))
            .count(),
        1,
        "exactly one selected run attempt must have its jobs checked"
    );
    Ok(())
}

#[test]
fn rejects_wrong_source_event_and_branch_before_api_access() -> Result<(), Box<dyn Error>> {
    for mutate in [0, 1, 2, 3, 4] {
        let mut scenario = Scenario::valid();
        match mutate {
            0 => {
                scenario.source_sha = Some(OTHER_SHA.to_owned());
                scenario.authority_sha = Some(OTHER_SHA.to_owned());
            }
            1 => scenario.event = "pull_request".to_owned(),
            2 => scenario.ref_name = "refs/heads/feature".to_owned(),
            3 => scenario.repository = "fork/repo-scan".to_owned(),
            _ => {
                scenario.workflow_ref =
                    "fork/repo-scan/.github/workflows/binary-release.yml@refs/heads/stable"
                        .to_owned();
            }
        }
        let result = execute(&scenario)?;
        assert_rejected(&result);
        assert!(
            result.calls.is_empty(),
            "identity rejection reached GitHub API"
        );
    }

    let mut wrong_authority = Scenario::valid();
    wrong_authority.authority_sha = Some(OTHER_SHA.to_owned());
    let result = execute(&wrong_authority)?;
    assert_rejected(&result);
    assert!(result.calls.is_empty());
    Ok(())
}

#[test]
fn ignores_ci_runs_with_wrong_source_event_or_branch_and_times_out() -> Result<(), Box<dyn Error>> {
    let wrong_runs = [
        ci_run(
            42,
            12,
            3,
            OTHER_SHA,
            DEFAULT_BRANCH,
            "push",
            ("completed", "success"),
        ),
        ci_run(
            42,
            12,
            3,
            SOURCE_TOKEN,
            DEFAULT_BRANCH,
            "pull_request",
            ("completed", "success"),
        ),
        ci_run(
            42,
            12,
            3,
            SOURCE_TOKEN,
            "feature",
            "push",
            ("completed", "success"),
        ),
    ];
    for wrong_run in wrong_runs {
        let mut scenario = Scenario::valid();
        scenario.runs = pages(&[vec![wrong_run]]);
        let result = execute(&scenario)?;
        assert_rejected(&result);
        assert!(
            result
                .stderr
                .contains("did not become successful before timeout")
        );
        assert_eq!(
            result
                .calls
                .lines()
                .filter(|line| line.ends_with("actions/workflows/ci.yml/runs"))
                .count(),
            2,
            "polling must stop at the configured limit"
        );
        assert!(!result.calls.contains("/attempts/"));
    }
    Ok(())
}

#[test]
fn rejects_missing_pending_and_failed_latest_runs() -> Result<(), Box<dyn Error>> {
    let mut missing = Scenario::valid();
    missing.runs = pages(&[vec![]]);
    let result = execute(&missing)?;
    assert_rejected(&result);
    assert!(
        result
            .stderr
            .contains("did not become successful before timeout")
    );
    assert_eq!(
        result
            .calls
            .lines()
            .filter(|line| line.ends_with("actions/workflows/ci.yml/runs"))
            .count(),
        2
    );

    let mut pending = Scenario::valid();
    pending.runs = pages(&[vec![ci_run(
        42,
        12,
        3,
        SOURCE_TOKEN,
        DEFAULT_BRANCH,
        "push",
        ("in_progress", ""),
    )]]);
    let result = execute(&pending)?;
    assert_rejected(&result);
    assert!(
        result
            .stderr
            .contains("did not become successful before timeout")
    );

    let mut failed = Scenario::valid();
    failed.runs = pages(&[vec![ci_run(
        42,
        12,
        3,
        SOURCE_TOKEN,
        DEFAULT_BRANCH,
        "push",
        ("completed", "failure"),
    )]]);
    let result = execute(&failed)?;
    assert_rejected(&result);
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run did not succeed")
    );
    Ok(())
}

#[test]
fn rejects_duplicate_mismatched_or_failed_required_jobs() -> Result<(), Box<dyn Error>> {
    let bad_jobs = [
        job_pages(&[]),
        job_pages(&[vec![
            required_job(42, 3, SOURCE_TOKEN, DEFAULT_BRANCH, "completed", "success"),
            required_job(42, 3, SOURCE_TOKEN, DEFAULT_BRANCH, "completed", "success"),
        ]]),
        job_pages(&[vec![required_job(
            41,
            3,
            SOURCE_TOKEN,
            DEFAULT_BRANCH,
            "completed",
            "success",
        )]]),
        job_pages(&[vec![required_job(
            42,
            2,
            SOURCE_TOKEN,
            DEFAULT_BRANCH,
            "completed",
            "success",
        )]]),
        job_pages(&[vec![required_job(
            42,
            3,
            OTHER_SHA,
            DEFAULT_BRANCH,
            "completed",
            "success",
        )]]),
        job_pages(&[vec![required_job(
            42,
            3,
            SOURCE_TOKEN,
            "feature",
            "completed",
            "success",
        )]]),
        job_pages(&[vec![required_job(
            42,
            3,
            SOURCE_TOKEN,
            DEFAULT_BRANCH,
            "completed",
            "failure",
        )]]),
        job_pages(&[vec![required_job(
            42,
            3,
            SOURCE_TOKEN,
            DEFAULT_BRANCH,
            "in_progress",
            "",
        )]]),
    ];
    for jobs in bad_jobs {
        let mut scenario = Scenario::valid();
        scenario.jobs = jobs;
        let result = execute(&scenario)?;
        assert_rejected(&result);
        assert!(result.calls.contains("actions/runs/42/attempts/"));
    }
    Ok(())
}

#[test]
fn rejects_a_default_branch_tip_mismatch_before_eligibility() -> Result<(), Box<dyn Error>> {
    let mut initially_moved = Scenario::valid();
    initially_moved.tip = format!(r#"[{{"sha":"{OTHER_SHA}"}}]"#);
    let result = execute(&initially_moved)?;
    assert_rejected(&result);
    assert!(
        result
            .stderr
            .contains("source is no longer the default branch tip")
    );
    assert!(!result.calls.contains("actions/workflows/ci.yml/runs"));

    let mut moved_during_check = Scenario::valid();
    moved_during_check.second_tip = Some(format!(r#"[{{"sha":"{OTHER_SHA}"}}]"#));
    let result = execute(&moved_during_check)?;
    assert_rejected(&result);
    assert!(
        result
            .stderr
            .contains("source is no longer the default branch tip")
    );
    Ok(())
}

#[test]
fn rejects_changes_to_latest_run_or_attempt_during_the_gate() -> Result<(), Box<dyn Error>> {
    let changes = [
        (
            pages(&[vec![ci_run(
                43,
                13,
                3,
                SOURCE_TOKEN,
                DEFAULT_BRANCH,
                "push",
                ("completed", "success"),
            )]]),
            "latest CI run changed during eligibility check",
        ),
        (
            pages(&[vec![ci_run(
                42,
                12,
                4,
                SOURCE_TOKEN,
                DEFAULT_BRANCH,
                "push",
                ("completed", "success"),
            )]]),
            "latest CI attempt changed during eligibility check",
        ),
    ];
    for (second_runs, expected_error) in changes {
        let mut scenario = Scenario::valid();
        scenario.second_runs = Some(second_runs);
        let result = execute(&scenario)?;
        assert_rejected(&result);
        assert!(result.stderr.contains(expected_error), "{}", result.stderr);
    }
    Ok(())
}

#[test]
fn rejects_default_branch_api_mismatch_and_bounds_all_gh_calls() -> Result<(), Box<dyn Error>> {
    let mut changed_default = Scenario::valid();
    changed_default.default_branch_response = r#"{"default_branch":"main"}"#.to_owned();
    let result = execute(&changed_default)?;
    assert_rejected(&result);
    assert!(result.stderr.contains("repository default branch changed"));
    assert_eq!(result.calls.lines().count(), 1);
    Ok(())
}
