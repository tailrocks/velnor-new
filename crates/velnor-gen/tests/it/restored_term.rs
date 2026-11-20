// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use crate::golden::samples;

type TestResult = Result<(), Box<dyn Error>>;

fn workflows() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut found = Vec::new();
    for sample in fs::read_dir(samples())? {
        let directory = sample?.path().join("expected/.github/workflows");
        for workflow in fs::read_dir(directory)? {
            let path = workflow?.path();
            found.push((path.display().to_string(), fs::read_to_string(&path)?));
        }
    }
    Ok(found)
}

fn referenced(condition: &str) -> BTreeSet<&str> {
    condition
        .split("steps.")
        .skip(1)
        .map(|rest| {
            rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .next()
                .unwrap_or_default()
        })
        .collect()
}

fn jobs(text: &str) -> Vec<(&str, Vec<&str>)> {
    let mut jobs: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line == "jobs:" {
            inside = true;
        } else if let Some(id) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|rest| inside && !rest.starts_with(' '))
        {
            jobs.push((id, Vec::new()));
        } else if let Some((_, lines)) = jobs.last_mut() {
            lines.push(line.trim_start().trim_start_matches("- "));
        }
    }
    jobs
}

fn dangling(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (id, lines) in jobs(text) {
        let ids: BTreeSet<&str> = lines
            .iter()
            .filter_map(|line| line.strip_prefix("id: "))
            .collect();
        for condition in lines.iter().filter_map(|line| line.strip_prefix("if: ")) {
            for used in referenced(condition) {
                if !ids.contains(used) {
                    found.push(format!(
                        "{id}: `if` reads steps.{used}, which the job lacks"
                    ));
                }
            }
        }
    }
    found
}

#[test]
fn no_step_condition_of_a_golden_job_reads_a_step_the_job_lacks() -> TestResult {
    let all = workflows()?;
    assert!(all.len() > 1);
    let found: Vec<String> = all
        .iter()
        .flat_map(|(path, text)| {
            dangling(text)
                .into_iter()
                .map(move |line| format!("{path} {line}"))
        })
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
    Ok(())
}

#[test]
fn the_rule_flags_a_condition_that_reads_a_missing_step() {
    let text = "jobs:\n  policy:\n    steps:\n      - run: a\n      - run: b\n        if: ${{ !cancelled() && steps.restored.outcome == 'success' }}\n  test:\n    steps:\n      - id: restored\n        run: c\n      - run: d\n        if: ${{ steps.restored.outcome == 'success' }}\n";
    assert_eq!(dangling(text).len(), 1);
}

#[test]
fn the_policy_lane_runs_its_policy_steps_without_a_restore_term() -> TestResult {
    let path = samples().join("policy-lane/expected/.github/workflows/rust-ci.yml");
    let text = fs::read_to_string(path)?;
    let (_, lines) = jobs(&text)
        .into_iter()
        .find(|(id, _)| *id == "policy")
        .ok_or("no policy job")?;
    let runs: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("run: "))
        .collect();
    assert!(
        runs.iter().any(|run| run.contains("cargo-deny")),
        "{runs:?}"
    );
    assert!(
        runs.iter().any(|run| run.contains("cargo machete")),
        "{runs:?}"
    );
    assert!(
        lines.iter().all(|line| !line.contains("restored")),
        "{lines:?}"
    );
    Ok(())
}
