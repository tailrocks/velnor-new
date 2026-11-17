// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, steps};
use crate::facts::TestResult;

const TOKENS: [&str; 3] = ["GITHUB_TOKEN", "github.token", "secrets."];

fn runs_repository_code(step: &str) -> bool {
    step.lines()
        .any(|line| line.trim().starts_with("run:") && line.contains("cargo"))
}

#[test]
fn no_step_holds_a_token_after_cargo_ran_repository_code_in_the_same_job() -> TestResult {
    for config in ["", "shape = \"lint-test\"\n"] {
        for (id, block) in jobs("0.1.81", config)? {
            let mut executed = false;
            for step in steps(&block) {
                assert!(
                    !executed || !TOKENS.iter().any(|token| step.contains(token)),
                    "{id} gives a token to a step after repository code ran:\n{step}"
                );
                executed |= runs_repository_code(step);
            }
        }
    }
    Ok(())
}

#[test]
fn the_upload_verification_runs_in_the_cache_job_that_runs_no_repository_code() -> TestResult {
    let all = jobs("0.1.81", "")?;
    let block = |wanted: &str| {
        all.iter()
            .find(|(id, _)| id == wanted)
            .map(|(_, block)| block.as_str())
            .ok_or("missing job")
    };
    assert!(!block("rust")?.contains("cache verify"));
    let trim = block("rust-trim")?;
    assert!(trim.contains("velnor-ci cache verify"), "{trim}");
    assert!(
        !trim.contains("checkout") && !trim.contains("cargo"),
        "{trim}"
    );
    Ok(())
}
