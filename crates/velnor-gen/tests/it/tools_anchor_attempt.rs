// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::tools::tools_anchor::{HELPER, PREFIX, STORES, jobs, render, steps, text};

const ATTEMPT_RUN: &str = "${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";
const PLAIN_RUN: &str = "${{ github.run_id }}-${{ github.job }}";
const ON_DEFAULT_BRANCH: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

fn anchor_steps(helper: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], STORES[0], helper)?;
    let mut found = Vec::new();
    for (_, job) in jobs(&yaml)? {
        found.extend(
            steps(&job)
                .into_iter()
                .filter(|step| step["name"] == "Find the anchor" || step["name"] == "Save anchor"),
        );
    }
    Ok(found)
}

#[test]
fn a_rerun_of_a_job_names_its_anchor_by_run_attempt_from_helper_0_1_112() -> TestResult {
    let before = anchor_steps(HELPER)?;
    assert!(
        before.iter().any(|step| step["name"] == "Save anchor"),
        "{before:?}"
    );
    for step in &before {
        assert_eq!(step["with"]["key"], format!("{PREFIX}{PLAIN_RUN}"));
    }
    let after = anchor_steps("0.1.112")?;
    assert_eq!(after.len(), before.len(), "{after:?}");
    for step in &after {
        assert_eq!(step["with"]["key"], format!("{PREFIX}{ATTEMPT_RUN}"));
    }
    Ok(())
}

#[test]
fn a_new_anchor_is_saved_only_on_the_default_branch() -> TestResult {
    for helper in [HELPER, "0.1.112"] {
        let saves: Vec<Value> = anchor_steps(helper)?
            .into_iter()
            .filter(|step| step["name"] == "Save anchor")
            .collect();
        assert_ne!(saves.len(), 0, "{helper}");
        for step in saves {
            assert!(text(&step, "if").contains(ON_DEFAULT_BRANCH), "{helper}");
        }
    }
    Ok(())
}
