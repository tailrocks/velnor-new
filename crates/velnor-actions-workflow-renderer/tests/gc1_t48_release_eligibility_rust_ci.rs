// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::gc1_t48_rust_ci_gate_fake::{
    CI_AMBIGUOUS, CI_ELIGIBLE, FAIL_CLOSED_CASES, RUST_AMBIGUOUS, RUST_ELIGIBLE, rendered_workflow,
    run_gate_script, run_gate_script_with_ci,
};
use super::*;

fn eligibility_script() -> Result<String, Box<dyn Error>> {
    rendered_workflow(".github/workflows/product-release.yml")?["jobs"]["release-eligibility"]
        ["steps"]
        .as_array()
        .ok_or_else(|| fail("eligibility steps missing"))?
        .iter()
        .find(|step| step["id"] == "check")
        .and_then(|step| step["run"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| fail("eligibility check step missing"))
}

#[test]
fn release_needs_a_green_rust_required_job_on_the_same_sha_beside_ci_required()
-> Result<(), Box<dyn Error>> {
    let script = eligibility_script()?;
    let (ok, stderr, output) = run_gate_script(&script, "green", "success")?;
    assert!(ok, "both green must be eligible: {stderr}");
    assert!(output.contains("ci_run_id=101"), "{output}");
    for (run, job, reason) in FAIL_CLOSED_CASES {
        let (ok, stderr, output) = run_gate_script(&script, run, job)?;
        assert!(!ok, "rust-ci {run}/{job} must not be eligible");
        assert!(stderr.contains(reason), "{run}/{job}: {stderr}");
        assert_eq!(output, "", "{run}/{job}: no outputs on failure");
    }
    Ok(())
}

#[test]
fn a_skipped_required_job_is_ignored_and_two_live_ones_are_ambiguous() -> Result<(), Box<dyn Error>>
{
    let script = eligibility_script()?;
    for ci in CI_ELIGIBLE {
        let (ok, stderr, output) = run_gate_script_with_ci(&script, "green", "success", ci)?;
        assert!(ok, "ci Required {ci} must be eligible: {stderr}");
        assert!(output.contains("ci_run_id=101"), "{ci}: {output}");
    }
    for rust in RUST_ELIGIBLE {
        let (ok, stderr, _) = run_gate_script(&script, "green", rust)?;
        assert!(ok, "rust-required {rust} must be eligible: {stderr}");
    }
    for ci in CI_AMBIGUOUS {
        let (ok, stderr, output) = run_gate_script_with_ci(&script, "green", "success", ci)?;
        assert!(!ok, "ci Required {ci} must not be eligible");
        assert!(stderr.contains("Required job"), "{ci}: {stderr}");
        assert_eq!(output, "", "{ci}: no outputs on failure");
    }
    for rust in RUST_AMBIGUOUS {
        let (ok, stderr, output) = run_gate_script(&script, "green", rust)?;
        assert!(!ok, "rust-required {rust} must not be eligible");
        assert!(
            stderr.contains("rust-required job response is invalid"),
            "{rust}: {stderr}"
        );
        assert_eq!(output, "", "{rust}: no outputs on failure");
    }
    Ok(())
}
