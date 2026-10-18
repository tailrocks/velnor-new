//! Preview admission reuses exact-source CI and freshness proofs with its own authority.
use super::support::{
    DEFAULT_BRANCH, OTHER_SHA, REPOSITORY, SOURCE_TOKEN, Scenario, assert_rejected, ci_run,
    execute_preview, pages,
};
use std::error::Error;

fn preview() -> Scenario {
    let mut scenario = Scenario::valid();
    scenario.event = "push".to_owned();
    scenario.workflow_ref =
        format!("{REPOSITORY}/.github/workflows/binary-preview.yml@refs/heads/{DEFAULT_BRANCH}");
    scenario
}

#[test]
fn preview_push_requires_exact_successful_ci_attempt() -> Result<(), Box<dyn Error>> {
    let result = execute_preview(&preview())?;
    assert!(result.success, "{}", result.stderr);
    assert!(result.output.contains("ci_run_id=42\nci_attempt=3\n"));
    assert!(result.calls.contains("actions/runs/42/attempts/3/jobs"));
    Ok(())
}

#[test]
fn preview_rejects_fork_events_and_alternate_workflow_authority() -> Result<(), Box<dyn Error>> {
    for event in [
        "pull_request",
        "pull_request_target",
        "workflow_run",
        "repository_dispatch",
    ] {
        let mut scenario = preview();
        scenario.event = event.to_owned();
        let result = execute_preview(&scenario)?;
        assert_rejected(&result);
        assert_eq!(result.calls.len(), 0);
    }
    let mut scenario = preview();
    scenario.workflow_ref = Scenario::valid().workflow_ref;
    let result = execute_preview(&scenario)?;
    assert_rejected(&result);
    assert_eq!(result.calls.len(), 0);
    Ok(())
}

#[test]
fn preview_rejects_failed_or_wrong_event_ci() -> Result<(), Box<dyn Error>> {
    for (event, conclusion) in [("push", "failure"), ("pull_request", "success")] {
        let mut scenario = preview();
        scenario.runs = pages(&[vec![ci_run(
            42,
            12,
            3,
            SOURCE_TOKEN,
            DEFAULT_BRANCH,
            event,
            ("completed", conclusion),
        )]]);
        assert_rejected(&execute_preview(&scenario)?);
    }
    Ok(())
}

#[test]
fn preview_rejects_stale_source_and_changed_ci_attempt() -> Result<(), Box<dyn Error>> {
    let mut stale = preview();
    stale.tip = format!(r#"[{{"sha":"{OTHER_SHA}"}}]"#);
    assert_rejected(&execute_preview(&stale)?);
    let mut changed_attempt = preview();
    changed_attempt.second_runs = Some(pages(&[vec![ci_run(
        42,
        12,
        4,
        SOURCE_TOKEN,
        DEFAULT_BRANCH,
        "push",
        ("completed", "success"),
    )]]));
    assert_rejected(&execute_preview(&changed_attempt)?);
    Ok(())
}

#[test]
fn recovery_admission_still_requires_exact_current_authority_and_ci() -> Result<(), Box<dyn Error>>
{
    // This gate supplies source admission only. Publication must separately
    // authenticate the retained original signed run and reject a new build.
    let mut recovery = preview();
    recovery.event = "workflow_dispatch".to_owned();
    let accepted = execute_preview(&recovery)?;
    assert!(accepted.success, "{}", accepted.stderr);
    recovery.authority_sha = Some(OTHER_SHA.to_owned());
    assert_rejected(&execute_preview(&recovery)?);
    Ok(())
}
