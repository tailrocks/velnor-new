// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use serde_json::Value;

use crate::facts::TestResult;
use crate::golden::{sample_dirs, samples};

const SECRET_MARKERS: [&str; 4] = ["GITHUB_TOKEN", "GH_TOKEN", "github.token", "secrets."];
const VERDICT_MARKERS: [&str; 2] = ["velnor-ci verify", "cache verify"];

fn text(value: &Value) -> String {
    value.to_string()
}

fn runs_repository_code(step: &Value) -> bool {
    let run = step["run"].as_str().unwrap_or_default();
    run.contains("$VELNOR_EXEC\" run ")
        || run.contains("cargo")
        || run.contains("mise run")
        || step["uses"].as_str().is_some_and(|u| u.starts_with("./"))
}

fn needs_a_clean_job(step: &Value, job: &Value) -> Option<&'static str> {
    let own = text(step);
    if SECRET_MARKERS.iter().any(|m| own.contains(m)) || text(&job["env"]).contains("secrets.") {
        return Some("carries a token");
    }
    let run = step["run"].as_str().unwrap_or_default();
    if VERDICT_MARKERS.iter().any(|m| run.contains(m)) {
        return Some("produces a verdict");
    }
    None
}

pub(crate) fn violations(workflow: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let parsed: Value = serde_saphyr::from_str(workflow)?;
    let mut found = Vec::new();
    for (id, job) in parsed["jobs"].as_object().ok_or("jobs")? {
        let mut executed = false;
        for step in job["steps"].as_array().map_or(&[][..], Vec::as_slice) {
            if executed && let Some(why) = needs_a_clean_job(step, job) {
                found.push(format!("{id}: {} {why}", step["name"]));
            }
            executed |= runs_repository_code(step);
        }
    }
    Ok(found)
}

#[test]
fn no_token_or_verdict_step_follows_repository_code_in_the_same_job() -> TestResult {
    let mut all = Vec::new();
    for dir in sample_dirs(&samples())? {
        let workflows = dir.join("expected/.github/workflows");
        let Ok(entries) = fs::read_dir(&workflows) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            for found in violations(&fs::read_to_string(&path)?)? {
                all.push(format!("{}: {found}", path.display()));
            }
        }
    }
    assert!(all.is_empty(), "{}", all.join("\n"));
    Ok(())
}

#[test]
fn a_token_step_after_a_cargo_step_is_a_violation() -> TestResult {
    let workflow = "jobs:\n  j:\n    steps:\n      - name: build\n        run: cargo build\n      - name: publish\n        env:\n          GITHUB_TOKEN: ${{ github.token }}\n        run: echo\n";
    assert_eq!(
        violations(workflow)?,
        vec!["j: \"publish\" carries a token".to_owned()]
    );
    Ok(())
}

#[test]
fn a_verdict_step_injected_after_the_tests_of_a_generated_workflow_is_a_violation() -> TestResult {
    let path = samples().join("two-roots/expected/.github/workflows/rust-ci.yml");
    let marker = "      - name: Upload cache metrics\n";
    let injected = fs::read_to_string(path)?.replacen(
        marker,
        &format!("      - name: Verify\n        run: '. \"$VELNOR_EXEC\" cache verify'\n{marker}"),
        1,
    );
    assert_eq!(
        violations(&injected)?,
        vec!["rust-root: \"Verify\" produces a verdict".to_owned()]
    );
    Ok(())
}
