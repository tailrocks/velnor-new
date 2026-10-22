use super::*;

#[test]
fn rejects_missing_duplicate_failed_and_wrong_attempt_required_jobs() -> Result<(), Box<dyn Error>>
{
    for jobs in [
        job_pages(&[]),
        job_pages(&[
            required_job(42, 3, SHA, "main", "completed", "success"),
            required_job(42, 3, SHA, "main", "completed", "success"),
        ]),
        job_pages(&[
            required_job(42, 3, SHA, "main", "completed", "success"),
            required_job(42, 2, SHA, "main", "completed", "success"),
        ]),
        job_pages(&[required_job(42, 3, SHA, "main", "completed", "failure")]),
        job_pages(&[required_job(42, 3, SHA, "other", "completed", "success")]),
        job_pages(&[required_job(42, 2, SHA, "main", "completed", "success")]),
    ] {
        let mut scenario = Scenario::valid();
        scenario.jobs = jobs;
        assert!(!execute(scenario)?.success);
    }
    Ok(())
}

#[test]
fn fails_closed_on_changed_tip_and_unfinished_latest_run() -> Result<(), Box<dyn Error>> {
    let mut changed_tip = Scenario::valid();
    changed_tip.main_sha = "fedcba9876543210fedcba9876543210fedcba98".to_owned();
    let result = execute(changed_tip)?;
    assert!(!result.success);
    assert!(result.stderr.contains("source is no longer the main tip"));

    let mut unfinished = Scenario::valid();
    unfinished.runs = pages(&[vec![run(
        42,
        12,
        3,
        SHA,
        "in_progress",
        "",
        ".github/workflows/ci.yml",
    )]]);
    let result = execute(unfinished)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run is not complete"),
        "{}",
        result.stderr
    );
    Ok(())
}

#[test]
fn rejects_events_repository_refs_authority_and_stale_ci_sources() -> Result<(), Box<dyn Error>> {
    for mutate in [0, 1, 2, 3, 4] {
        let mut scenario = Scenario::valid();
        match mutate {
            0 => scenario.event = "workflow_run".to_owned(),
            1 => scenario.repository = "fork/velnor-new".to_owned(),
            2 => scenario.ref_name = "refs/heads/release".to_owned(),
            3 => {
                scenario.workflow_ref =
                    "fork/velnor-new/.github/workflows/product-release.yml@refs/heads/main"
                        .to_owned();
            }
            _ => scenario.authority_sha = "fedcba9876543210fedcba9876543210fedcba98".to_owned(),
        }
        let result = execute(scenario)?;
        assert!(!result.success);
        assert_eq!(result.calls.len(), 0);
    }

    let mut stale = Scenario::valid();
    stale.runs = pages(&[vec![run(
        42,
        12,
        3,
        "fedcba9876543210fedcba9876543210fedcba98",
        "completed",
        "success",
        ".github/workflows/ci.yml",
    )]]);
    let result = execute(stale)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run is missing"),
        "{}",
        result.stderr
    );
    Ok(())
}

#[test]
fn rejects_ci_run_from_wrong_head_repository() -> Result<(), Box<dyn Error>> {
    let mut scenario = Scenario::valid();
    scenario.runs = scenario.runs.replace(REPO, "fork/velnor-new");
    let result = execute(scenario)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run is missing"),
        "{}",
        result.stderr
    );
    Ok(())
}

#[test]
fn rejects_ambiguous_latest_run_number() -> Result<(), Box<dyn Error>> {
    let mut scenario = Scenario::valid();
    scenario.runs = pages(&[vec![
        run(
            42,
            12,
            3,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        ),
        run(
            43,
            12,
            1,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        ),
    ]]);
    let result = execute(scenario)?;
    assert!(!result.success);
    Ok(())
}

#[test]
fn rejects_main_tip_or_ci_attempt_changes_during_the_gate() -> Result<(), Box<dyn Error>> {
    let mut changed_tip = Scenario::valid();
    changed_tip.second_main_sha = Some("fedcba9876543210fedcba9876543210fedcba98".to_owned());
    let result = execute(changed_tip)?;
    assert!(!result.success);
    assert!(result.stderr.contains("source is no longer the main tip"));

    let mut changed_attempt = Scenario::valid();
    changed_attempt.second_runs = Some(pages(&[vec![run(
        42,
        12,
        4,
        SHA,
        "completed",
        "success",
        ".github/workflows/ci.yml",
    )]]));
    let result = execute(changed_attempt)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest CI attempt changed during eligibility check")
    );
    Ok(())
}

fn listing_calls(result: &Execution) -> usize {
    result
        .calls
        .lines()
        .filter(|line| line.contains("/actions/workflows/ci.yml/runs?"))
        .count()
}

#[test]
fn recovers_when_github_briefly_lists_no_runs() -> Result<(), Box<dyn Error>> {
    // GitHub intermittently answers an exact head_sha listing with no runs.
    // Call 1 is the first lookup and call 2 the recheck; each retries once.
    for (call, empty) in [(1, pages(&[vec![]])), (2, "[]".to_owned())] {
        let mut scenario = Scenario::valid();
        scenario.runs_by_call = vec![(call, empty)];
        let result = execute(scenario)?;
        assert!(result.success, "call {call}: {}", result.stderr);
        assert!(result.output.contains("ci_run_id=42"));
        assert!(result.output.contains("ci_attempt=3"));
        assert_eq!(result.sleeps, "3\n");
        assert_eq!(listing_calls(&result), 3);
        assert!(
            result
                .stderr
                .contains("release eligibility: the CI run listing is empty; listing again in 3s")
        );
    }
    Ok(())
}

#[test]
fn fails_closed_when_the_listing_stays_empty() -> Result<(), Box<dyn Error>> {
    let mut scenario = Scenario::valid();
    scenario.runs = pages(&[vec![]]);
    let result = execute(scenario)?;
    assert!(!result.success);
    assert_eq!(result.output, "");
    assert!(result.stderr.contains("stayed empty after 6 attempts"));
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run is missing")
    );
    assert_eq!(result.sleeps, "3\n6\n12\n24\n45\n");
    assert_eq!(listing_calls(&result), 6);
    assert!(!result.calls.contains("/attempts/"));
    Ok(())
}

#[test]
fn never_retries_a_mismatching_listing() -> Result<(), Box<dyn Error>> {
    let other_sha = "fedcba9876543210fedcba9876543210fedcba98";
    let mismatches = [
        run(
            42,
            12,
            3,
            other_sha,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        ),
        run(
            42,
            12,
            3,
            SHA,
            "completed",
            "success",
            ".github/workflows/other.yml",
        ),
        run(
            42,
            12,
            3,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        )
        .replace(r#""event":"push""#, r#""event":"pull_request""#),
        run(
            42,
            12,
            3,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        )
        .replace(r#""head_branch":"main""#, r#""head_branch":"topic""#),
        run(
            42,
            12,
            3,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        )
        .replace(REPO, "fork/velnor-new"),
    ];
    for mismatch in mismatches {
        let mut scenario = Scenario::valid();
        scenario.runs = pages(&[vec![mismatch.clone()]]);
        let result = execute(scenario)?;
        assert!(!result.success, "{mismatch}");
        assert!(
            result
                .stderr
                .contains("latest exact-source CI run is missing"),
            "{mismatch}: {}",
            result.stderr
        );
        assert_eq!(result.sleeps, "", "{mismatch}");
        assert_eq!(listing_calls(&result), 1, "{mismatch}");
    }
    Ok(())
}

#[test]
fn never_retries_a_mismatching_recheck_listing() -> Result<(), Box<dyn Error>> {
    // A recheck that lists only a mismatching run fails at once as well.
    let other_sha = "fedcba9876543210fedcba9876543210fedcba98";
    let mut recheck = Scenario::valid();
    recheck.second_runs = Some(pages(&[vec![run(
        42,
        12,
        3,
        other_sha,
        "completed",
        "success",
        ".github/workflows/ci.yml",
    )]]));
    let result = execute(recheck)?;
    assert!(!result.success);
    assert!(result.stderr.contains("latest CI recheck ID is invalid"));
    assert_eq!(result.sleeps, "");
    assert_eq!(listing_calls(&result), 2);
    Ok(())
}

#[test]
fn never_retries_a_failed_or_unreadable_listing() -> Result<(), Box<dyn Error>> {
    let mut failed = Scenario::valid();
    failed.runs_by_call = vec![(1, FAIL.to_owned())];
    let result = execute(failed)?;
    assert!(!result.success);
    assert!(result.stderr.contains("latest CI run lookup failed"));
    assert_eq!(result.sleeps, "");
    assert_eq!(listing_calls(&result), 1);

    let mut unreadable = Scenario::valid();
    unreadable.runs = r#"{"message":"Server Error"}"#.to_owned();
    let result = execute(unreadable)?;
    assert!(!result.success);
    assert!(result.stderr.contains("CI run response is invalid"));
    assert_eq!(result.sleeps, "");
    assert_eq!(listing_calls(&result), 1);
    Ok(())
}
