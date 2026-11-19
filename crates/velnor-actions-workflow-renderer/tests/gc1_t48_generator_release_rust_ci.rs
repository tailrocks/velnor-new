// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::gc1_t48_rust_ci_gate_fake::{FAIL_CLOSED_CASES, rendered_workflow, run_gate_script};
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
