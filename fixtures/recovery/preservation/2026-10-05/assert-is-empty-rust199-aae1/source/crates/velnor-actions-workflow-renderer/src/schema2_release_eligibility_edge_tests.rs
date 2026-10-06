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
            .contains("did not become successful before timeout")
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
        assert_eq!(result.calls, "");
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
            .contains("did not become successful before timeout")
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
            .contains("did not become successful before timeout")
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
