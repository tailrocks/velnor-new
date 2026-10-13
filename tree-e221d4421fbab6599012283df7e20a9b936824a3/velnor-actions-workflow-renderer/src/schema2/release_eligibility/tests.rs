use super::{ELIGIBILITY_SCRIPT, JOB_ID, REQUIRED_JOB_JQ, job, script};
use std::error::Error;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const AUTHORITY: &str = SHA;
const REPO: &str = "tailrocks/velnor-new";
const WORKFLOW: &str = "tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main";

mod edge;
mod harness;

use harness::*;

#[test]
fn selects_latest_ci_attempt_across_paginated_results() -> Result<(), Box<dyn Error>> {
    let result = execute(Scenario::valid())?;
    assert!(result.success, "{}", result.stderr);
    assert!(result.output.contains(&format!("source_sha={SHA}")));
    assert!(
        result
            .output
            .contains(&format!("workflow_authority_sha={AUTHORITY}"))
    );
    assert!(result.output.contains("ci_run_id=42"));
    assert!(result.output.contains("ci_attempt=3"));
    assert!(result.calls.contains("/attempts/3/jobs?per_page=100"));
    Ok(())
}

#[test]
fn rejects_newer_failed_run_instead_of_reusing_old_success() -> Result<(), Box<dyn Error>> {
    let mut scenario = Scenario::valid();
    scenario.runs = pages(&[
        vec![run(
            41,
            11,
            1,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        )],
        vec![run(
            42,
            12,
            1,
            SHA,
            "completed",
            "failure",
            ".github/workflows/ci.yml",
        )],
    ]);
    let result = execute(scenario)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run did not succeed")
    );
    assert!(!result.calls.contains("/attempts/"));
    Ok(())
}

#[test]
fn script_uses_the_expected_paginated_selectors_and_bounded_wait() {
    let rendered = script();
    assert!(!rendered.contains("@LATEST_RUN_JQ@"));
    assert!(!rendered.contains("@REQUIRED_JOB_JQ@"));
    assert!(rendered.contains("/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100"));
    assert!(rendered.contains("--paginate --slurp"));
    assert!(rendered.contains("latest CI attempt changed during eligibility check"));
    assert!(rendered.contains("poll_limit=\"${VELNOR_RELEASE_CI_POLL_LIMIT:-240}\""));
    assert!(REQUIRED_JOB_JQ.contains(".name == \"Required\""));
    assert!(REQUIRED_JOB_JQ.contains(".run_attempt == $run_attempt"));
    assert!(rendered.contains("head_repository.full_name == $repository"));
    assert!(ELIGIBILITY_SCRIPT.contains("GITHUB_WORKFLOW_REF"));
}

#[test]
fn job_renders_a_read_only_source_gate() {
    let (name, value) = job(crate::yaml::Yaml::str("ubuntu-latest"));
    assert_eq!(name, JOB_ID);
    let rendered = crate::yaml::render_yaml(&crate::yaml::Yaml::Map(vec![(name, value)]));
    assert!(rendered.contains("actions: read"));
    assert!(rendered.contains("contents: read"));
    assert!(rendered.contains("gh@2.102.0"));
    assert!(rendered.contains("workflow_authority_sha"));
}
