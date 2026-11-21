// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::MAX_WORKFLOW_BYTES;

use crate::facts::TestResult;
use crate::newest_helper::newest_helper;
use crate::pipeline::{pin_helper, view, workflow};

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\nlint = true\ndoc = true\njob = 'parallel'\n";
const SAVED_AT_LEAST: usize = 10_000;
const PARALLEL_ROOTS: usize = 21;

fn roots(
    count: usize,
    config: Option<&str>,
    helper: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let manifests: Vec<String> = (0..count)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("crate-with-a-long-directory-name-{index}/Cargo.toml")
            }
        })
        .collect();
    let paths: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut repo = view(&paths, config.map(str::as_bytes))?;
    pin_helper(&mut repo, helper)?;
    Ok(workflow(&repo)?.len())
}

#[test]
fn one_anchor_read_keeps_a_twenty_root_workflow_smaller_than_one_read_per_job() -> TestResult {
    let per_job = roots(20, None, "0.1.109")?;
    let once = roots(20, None, "0.1.110")?;
    assert!(
        once + SAVED_AT_LEAST < per_job && once < MAX_WORKFLOW_BYTES,
        "{once} bytes against {per_job} with a read per job"
    );
    Ok(())
}

#[test]
fn a_twenty_one_root_parallel_workflow_with_a_read_per_job_is_refused_and_one_read_renders()
-> TestResult {
    assert!(roots(PARALLEL_ROOTS, Some(LANE), "0.1.109").is_err());
    let once = roots(PARALLEL_ROOTS, Some(LANE), &newest_helper()?)?;
    assert!(once < MAX_WORKFLOW_BYTES, "{once} bytes");
    Ok(())
}
