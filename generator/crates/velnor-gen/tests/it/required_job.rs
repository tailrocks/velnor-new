// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::{HELPER_LOCK, view, workflow};

fn released(version: &str, roots: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = view(roots, None)?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.79", version, 1);
    view.files
        .insert(RepoPath::new(".velnor/rust-ci.lock")?, lock.into_bytes());
    workflow(&view)
}

fn required(text: &str) -> Result<&str, &'static str> {
    text.find("  required:\n")
        .map(|at| &text[at..])
        .ok_or("no required job")
}

fn gate_line(text: &str) -> Result<&str, &'static str> {
    required(text)?
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: '"))
        .map(|line| line.trim_end_matches('\''))
        .ok_or("no gate line")
}

fn words(line: &str, flag: &str) -> Vec<String> {
    let all: Vec<&str> = line.split(' ').collect();
    all.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].to_owned())
        .collect()
}

#[test]
fn a_helper_that_has_gate_renders_the_gate_in_two_steps_without_a_checkout() -> TestResult {
    let text = released("0.1.81", &["Cargo.toml", "lab/Cargo.toml"])?;
    let job = required(&text)?;
    assert!(job.contains("    if: '${{ always() }}'\n"));
    assert!(!job.contains("Check out"));
    assert!(!job.contains("Fail when a Rust job failed or was cancelled"));
    assert_eq!(job.matches("      - name: ").count(), 2);
    assert!(job.contains("      - name: Install velnor-ci 0.1.81\n"));
    assert!(job.contains("        env:\n          NEEDS_JSON: '${{ toJSON(needs) }}'\n"));
    assert_eq!(
        gate_line(&text)?,
        "velnor-ci gate --plan rust-plan --job rust-plan=always --job rust-root=lane:root --job rust-lab=lane:lab --job rust-inputs-audit=schedule --tests rust-root=root --tests rust-lab=lab"
    );
    Ok(())
}

#[test]
fn a_helper_without_gate_keeps_the_old_verdict_step() -> TestResult {
    let text = released("0.1.80", &["Cargo.toml", "lab/Cargo.toml"])?;
    let job = required(&text)?;
    assert!(job.contains("    if: '${{ !cancelled() }}'\n"));
    assert!(job.contains("Fail when a Rust job failed or was cancelled"));
    assert!(!job.contains("velnor-ci gate"));
    assert!(!text.contains("_tests"));
    Ok(())
}

#[test]
fn the_plan_job_exports_the_tests_flag_of_every_lane_that_counts_tests() -> TestResult {
    let text = released("0.1.81", &["Cargo.toml", "lab/Cargo.toml"])?;
    for slug in ["root", "lab"] {
        assert!(text.contains(&format!(
            "      rust_{slug}_tests: '${{{{ steps.plan.outputs.rust_{slug}_tests }}}}'\n"
        )));
    }
    Ok(())
}

#[test]
fn one_job_exports_its_own_plan_flag_and_the_gate_reads_it_from_that_job() -> TestResult {
    let text = released("0.1.81", &["Cargo.toml"])?;
    let line = gate_line(&text)?;
    assert_eq!(
        line,
        "velnor-ci gate --plan rust --job rust=always --tests rust=root"
    );
    assert!(text.contains("      rust_root_tests: '${{ steps.plan.outputs.rust_root_tests }}'\n"));
    assert!(text.contains("      tests_run: '${{ steps.test.outputs.tests_run }}'\n"));
    Ok(())
}

#[test]
fn the_gate_names_every_job_the_verdict_job_needs() -> TestResult {
    let text = released(
        "0.1.81",
        &["Cargo.toml", "lab/Cargo.toml", "tools/Cargo.toml"],
    )?;
    let job = required(&text)?;
    let needs: Vec<&str> = job
        .lines()
        .skip_while(|line| *line != "    needs:")
        .skip(1)
        .take_while(|line| line.starts_with("      - "))
        .map(|line| line.trim_start_matches("      - "))
        .collect();
    let line = gate_line(&text)?;
    let named: Vec<String> = words(line, "--job")
        .iter()
        .filter_map(|job| job.split_once('=').map(|(id, _)| id.to_owned()))
        .collect();
    assert_eq!(needs, named);
    Ok(())
}
