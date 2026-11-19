// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::gc1_t48_rust_ci_gate_fake::{
    CI_AMBIGUOUS, CI_ELIGIBLE, FAIL_CLOSED_CASES, RUST_AMBIGUOUS, RUST_ELIGIBLE, rendered_workflow,
    run_gate_script, run_gate_script_with_ci,
};
use super::*;

fn source_check_script() -> Result<String, Box<dyn Error>> {
    rendered_workflow(".github/workflows/product-release-generator.yml")?["jobs"]
        ["verify-release-source"]["steps"]
        .as_array()
        .ok_or_else(|| fail("source gate steps missing"))?
        .iter()
        .find(|step| step["name"] == "Require successful CI at exact main SHA")
        .and_then(|step| step["run"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| fail("source gate CI step missing"))
}

#[test]
fn generator_release_source_needs_a_green_rust_required_job_beside_ci_required()
-> Result<(), Box<dyn Error>> {
    let script = source_check_script()?;
    let (ok, stderr, _) = run_gate_script(&script, "green", "success")?;
    assert!(ok, "both green must verify the source: {stderr}");
    for (run, job, reason) in FAIL_CLOSED_CASES {
        let (ok, stderr, _) = run_gate_script(&script, run, job)?;
        assert!(!ok, "rust-ci {run}/{job} must not verify the source");
        assert!(stderr.contains(reason), "{run}/{job}: {stderr}");
    }
    Ok(())
}

#[test]
fn generator_source_ignores_a_skipped_required_job_and_rejects_two_live_ones()
-> Result<(), Box<dyn Error>> {
    let script = source_check_script()?;
    for ci in CI_ELIGIBLE {
        let (ok, stderr, _) = run_gate_script_with_ci(&script, "green", "success", ci)?;
        assert!(ok, "ci Required {ci} must verify the source: {stderr}");
    }
    for rust in RUST_ELIGIBLE {
        let (ok, stderr, _) = run_gate_script(&script, "green", rust)?;
        assert!(ok, "rust-required {rust} must verify the source: {stderr}");
    }
    for ci in CI_AMBIGUOUS {
        let (ok, _, _) = run_gate_script_with_ci(&script, "green", "success", ci)?;
        assert!(!ok, "ci Required {ci} must not verify the source");
    }
    for rust in RUST_AMBIGUOUS {
        let (ok, stderr, _) = run_gate_script(&script, "green", rust)?;
        assert!(!ok, "rust-required {rust} must not verify the source");
        assert!(
            stderr.contains("rust-required job response is invalid"),
            "{rust}: {stderr}"
        );
    }
    Ok(())
}
