// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::MAX_WORKFLOW_BYTES;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\nlint = true\ndoc = true\njob = 'parallel'\n";
const SAVED_AT_LEAST: usize = 10_000;

fn twenty_roots(config: Option<&str>, helper: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let manifests: Vec<String> = (0..20)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("crate-with-a-long-directory-name-{index}/Cargo.toml")
            }
        })
        .collect();
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut repo = view(&roots, config.map(str::as_bytes))?;
    pin_helper(&mut repo, helper)?;
    Ok(workflow(&repo)?.len())
}

#[test]
fn one_anchor_read_keeps_a_twenty_root_workflow_smaller_than_one_read_per_job() -> TestResult {
    let per_job = twenty_roots(None, "0.1.109")?;
    let once = twenty_roots(None, "0.1.110")?;
    assert!(
        once + SAVED_AT_LEAST < per_job && once < MAX_WORKFLOW_BYTES,
        "{once} bytes against {per_job} with a read per job"
    );
    Ok(())
}

#[test]
fn a_twenty_root_parallel_workflow_with_a_read_per_job_is_refused_and_one_read_renders()
-> TestResult {
    assert!(twenty_roots(Some(LANE), "0.1.109").is_err());
    let once = twenty_roots(Some(LANE), "0.1.110")?;
    assert!(once < MAX_WORKFLOW_BYTES, "{once} bytes");
    Ok(())
}
