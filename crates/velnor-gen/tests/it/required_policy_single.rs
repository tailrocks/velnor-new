// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{JobId, RepoPath, VerdictSpec};
use velnor_rust::{Need, decide};

use crate::facts::TestResult;
use crate::pipeline::{HELPER_LOCK, view, workflow};

const MISE: &str = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-deny = \"0.18.4\"\ncargo-machete = \"0.9.1\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n";

fn rendered() -> Result<String, Box<dyn std::error::Error>> {
    let mut view = view(&["Cargo.toml"], None)?;
    for (path, bytes) in [
        ("deny.toml", Vec::new()),
        ("mise.toml", MISE.as_bytes().to_vec()),
        (
            ".velnor/rust-ci.lock",
            String::from_utf8(HELPER_LOCK.to_vec())?.into_bytes(),
        ),
    ] {
        let path = RepoPath::new(path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, bytes);
    }
    workflow(&view)
}

fn job<'a>(text: &'a str, id: &str) -> Result<&'a str, String> {
    let start = text
        .find(&format!("\n  {id}:\n"))
        .ok_or(format!("no job {id}"))?;
    let rest = &text[start + 1..];
    Ok(rest
        .match_indices("\n  ")
        .find(|(at, _)| rest[at + 3..].chars().next().is_some_and(|c| c != ' '))
        .map_or(rest, |(at, _)| &rest[..at]))
}

fn exported(text: &str, id: &str) -> Result<Vec<String>, String> {
    Ok(job(text, id)?
        .lines()
        .skip_while(|line| *line != "    outputs:")
        .skip(1)
        .take_while(|line| line.starts_with("      "))
        .filter_map(|line| line.trim().split_once(':').map(|(key, _)| key.to_owned()))
        .collect())
}

fn spec(text: &str) -> Result<VerdictSpec, String> {
    let line = text
        .split("GATE_SPEC: '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .ok_or("no gate spec")?;
    let words: Vec<&str> = line.split(' ').collect();
    let after = |flag: &str| -> Vec<String> {
        words
            .windows(2)
            .filter(|pair| pair[0] == flag)
            .map(|pair| pair[1].to_owned())
            .collect()
    };
    let plan = after("--plan").pop().ok_or("no plan")?;
    VerdictSpec::parse(&plan, &after("--job"), &after("--tests"))
}

fn verdict(
    text: &str,
    policy: &str,
    value: impl Fn(&str) -> Option<&'static str>,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let spec = spec(text)?;
    let outputs: BTreeMap<String, String> = exported(text, "rust")?
        .into_iter()
        .filter_map(|key| value(&key).map(|value| (key, value.to_owned())))
        .collect();
    let mut needs = BTreeMap::new();
    needs.insert(
        JobId::new("rust")?,
        Need {
            result: Some("success".to_owned()),
            outputs: outputs
                .into_iter()
                .chain([
                    ("tests_run".to_owned(), "3".to_owned()),
                    ("tests_listed".to_owned(), "3".to_owned()),
                    ("tests_by_package".to_owned(), "pkg=3/3".to_owned()),
                ])
                .collect(),
        },
    );
    needs.insert(
        JobId::new("policy")?,
        Need {
            result: Some(policy.to_owned()),
            outputs: BTreeMap::new(),
        },
    );
    let verdict = decide(&spec, &needs, Some("pull_request"));
    Ok((verdict.passed, verdict.table()))
}

#[test]
fn one_job_with_policy_enabled_exports_what_the_policy_gate_reads() -> TestResult {
    let text = rendered()?;
    assert!(
        spec(&text)?
            .jobs
            .iter()
            .any(|job| job.id.to_string() == "policy")
    );
    let (green, table) = verdict(&text, "success", |_| Some("run"))?;
    assert!(green, "{table}");
    let (green, table) = verdict(&text, "failure", |_| Some("run"))?;
    assert!(!green, "{table}");
    assert!(table.contains("policy"), "{table}");
    Ok(())
}

#[test]
fn a_policy_job_that_did_not_succeed_fails_the_gate() -> TestResult {
    let text = rendered()?;
    for result in ["skipped", "cancelled"] {
        let (green, table) = verdict(&text, result, |_| Some("run"))?;
        assert!(!green, "{table}");
    }
    Ok(())
}

#[test]
fn the_rust_job_exports_the_policy_outputs_the_gate_reads() -> TestResult {
    let text = rendered()?;
    let exported = exported(&text, "rust")?;
    for output in ["rust_deny", "rust_machete", "rust_root_tests"] {
        assert!(exported.contains(&output.to_owned()), "{exported:?}");
    }
    Ok(())
}

#[test]
fn a_run_that_skips_both_policy_tools_leaves_the_gate_green_with_a_skipped_policy_job() -> TestResult
{
    let text = rendered()?;
    let (green, table) = verdict(&text, "skipped", |key| {
        Some(if key.ends_with("_tests") {
            "run"
        } else {
            "skip"
        })
    })?;
    assert!(green, "{table}");
    Ok(())
}
