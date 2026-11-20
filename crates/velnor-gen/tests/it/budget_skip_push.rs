// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::RepoView;

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const BUDGET: &str = "[budget]\nmain_wall_seconds = 600\n";
const SKIP_PUSH: &str = "[ci]\nskip_push_after_merge_group = true\n\n";

fn repo(roots: &[&str], config: &str, helper: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = view(roots, Some(config.as_bytes()))?;
    crate::pipeline::pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn budget_job(
    roots: &[&str],
    config: &str,
    helper: &str,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let tree = crate::yaml_tree::document(&workflow(&repo(roots, config, helper)?)?)?;
    tree.pointer("/jobs/rust-budget")
        .cloned()
        .ok_or_else(|| "the budget job is missing".into())
}

fn condition(job: &serde_json::Value) -> Result<&str, Box<dyn Error>> {
    job["if"]
        .as_str()
        .ok_or_else(|| "the budget job has no condition".into())
}

#[test]
fn a_push_skipped_after_the_queue_run_passed_is_not_a_budget_sample() -> TestResult {
    let config = format!("{SKIP_PUSH}{BUDGET}");
    for (roots, producer) in [
        (&["Cargo.toml"][..], "rust"),
        (&["Cargo.toml", "lab/Cargo.toml"][..], "rust-plan"),
    ] {
        for helper in ["0.1.103", "0.1.104"] {
            let job = budget_job(roots, &config, helper)?;
            let when = condition(&job)?;
            assert!(
                when.contains(&format!("needs.{producer}.outputs.skip_push != 'true'")),
                "{producer} {helper}: a skipped push finishes in seconds and would drag the medians down\n{when}"
            );
            let needs: Vec<&str> = job["needs"]
                .as_array()
                .ok_or("needs")?
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect();
            assert!(
                needs.contains(&producer),
                "the job reads the output of {producer}: {needs:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn without_the_skip_the_budget_job_does_not_read_the_skip_output() -> TestResult {
    for roots in [&["Cargo.toml"][..], &["Cargo.toml", "lab/Cargo.toml"][..]] {
        let job = budget_job(roots, BUDGET, "0.1.104")?;
        let when = condition(&job)?;
        assert!(!when.contains("skip_push"), "{when}");
        assert!(!when.contains("merge_group.outputs.skip"), "{when}");
    }
    Ok(())
}
