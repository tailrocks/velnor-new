// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use super::support::rendered_job_block;
use super::waste_oracle::{
    Files, children, installs_toolchain, job_env_entries, job_ids, run_scalar,
};

const VERIFIER_NAMES: [&str; 2] = [
    "Verify Rust channel manifest",
    "Verify restored Rust toolchain",
];
const VERIFIER_ACTIONS: [&str; 2] = [
    "./.github/actions/velnor-verify-restored-rust",
    "./.github/actions/velnor-verify-rust-manifest",
];
const HOME_ENVS: [&str; 2] = ["RUSTUP_HOME", "CARGO_HOME"];

type Env = BTreeMap<String, String>;

#[derive(Debug)]
pub(super) struct StepText {
    pub(super) name: String,
    pub(super) uses: Option<String>,
    pub(super) run: Option<String>,
    env: Env,
    installs_rust: bool,
}

#[derive(Debug, Default)]
pub(super) struct VerifierScan {
    pub(super) checked: Vec<Checked>,
    pub(super) skipped: Vec<String>,
    pub(super) violations: Vec<String>,
}

#[derive(Debug)]
pub(super) struct Checked {
    pub(super) file: String,
    pub(super) job: String,
    pub(super) via_composite: bool,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

pub(super) fn steps_of(text: &str, label: &str) -> Result<Vec<StepText>, Box<dyn Error>> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(head) = lines
        .iter()
        .find(|line| line.trim_start().starts_with("- name:"))
        .map(|line| indent(line))
    else {
        return Ok(Vec::new());
    };
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| indent(line) == head && line.trim_start().starts_with("- name:"))
        .map(|(at, _)| at)
        .collect();
    let mut steps = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(lines.len());
        let body: Vec<&str> = lines[*start..end].to_vec();
        steps.push(step_of(&body, head + 2, label)?);
    }
    Ok(steps)
}

fn step_of(body: &[&str], field: usize, label: &str) -> Result<StepText, Box<dyn Error>> {
    let mut step = StepText {
        name: String::new(),
        uses: None,
        run: None,
        env: Env::new(),
        installs_rust: false,
    };
    for (at, line) in body.iter().enumerate() {
        let trimmed = if at == 0 {
            line.trim_start().trim_start_matches("- ")
        } else if indent(line) == field {
            line.trim()
        } else {
            continue;
        };
        if let Some(value) = trimmed.strip_prefix("name:") {
            value.trim().clone_into(&mut step.name);
        } else if let Some(value) = trimmed.strip_prefix("uses:") {
            step.uses = value.split_whitespace().next().map(str::to_owned);
        } else if trimmed == "env:" {
            step.env.extend(children(&body[at..].join("\n")));
        } else if let Some(value) = trimmed.strip_prefix("run:") {
            let scalar = run_scalar(body, at, value.trim(), label)?;
            step.installs_rust = installs_toolchain(&scalar);
            step.run = Some(scalar);
        }
    }
    Ok(step)
}

fn composite_env(files: &Files, step: &StepText) -> Result<Env, Box<dyn Error>> {
    let Some(name) = step
        .uses
        .as_deref()
        .and_then(|uses| uses.strip_prefix("./.github/actions/"))
    else {
        return Ok(Env::new());
    };
    let path = format!(".github/actions/{name}/action.yml");
    let text = files
        .get(&path)
        .ok_or_else(|| format!("local action {path} is missing"))?;
    Ok(steps_of(text, &path)?
        .into_iter()
        .flat_map(|inner| inner.env)
        .collect())
}

fn effective_env(files: &Files, job_env: &Env, step: &StepText) -> Result<Env, Box<dyn Error>> {
    let mut env = job_env.clone();
    env.extend(step.env.clone());
    env.extend(composite_env(files, step)?);
    Ok(env)
}

fn is_rust_verifier(step: &StepText) -> bool {
    VERIFIER_NAMES.contains(&step.name.as_str())
        || step
            .uses
            .as_deref()
            .is_some_and(|uses| VERIFIER_ACTIONS.contains(&uses))
}

fn unset_or(value: Option<&String>) -> &str {
    value.map_or("<unset>", String::as_str)
}

fn scan_job(
    files: &Files,
    file: &str,
    workflow: &str,
    id: &str,
    scan: &mut VerifierScan,
) -> Result<(), Box<dyn Error>> {
    let block = rendered_job_block(workflow, id)?;
    let steps = steps_of(block, &format!("{file} {id}"))?;
    let verifiers: Vec<&StepText> = steps.iter().filter(|step| is_rust_verifier(step)).collect();
    if verifiers.is_empty() {
        return Ok(());
    }
    let job_env: Env = job_env_entries(workflow, block)?.into_iter().collect();
    let mut installs = Vec::new();
    for step in steps.iter().filter(|step| step.installs_rust) {
        installs.push((step, effective_env(files, &job_env, step)?));
    }
    let Some((install, expected)) = installs
        .iter()
        .find(|(_, env)| HOME_ENVS.iter().all(|key| env.contains_key(*key)))
    else {
        scan.skipped.push(format!("{file} {id}"));
        return Ok(());
    };
    for verifier in verifiers {
        let env = effective_env(files, &job_env, verifier)?;
        scan.checked.push(Checked {
            file: file.to_owned(),
            job: id.to_owned(),
            via_composite: !composite_env(files, verifier)?.is_empty(),
        });
        for key in HOME_ENVS {
            if env.get(key) != expected.get(key) {
                scan.violations.push(format!(
                    "{file} {id}: {:?} resolves {key} to {}, but {:?} installs into {}",
                    verifier.name,
                    unset_or(env.get(key)),
                    install.name,
                    unset_or(expected.get(key)),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn verifier_homes(files: &Files) -> Result<VerifierScan, Box<dyn Error>> {
    let mut scan = VerifierScan::default();
    for (file, workflow) in files {
        if !file.starts_with(".github/workflows/") {
            continue;
        }
        for id in job_ids(workflow) {
            scan_job(files, file, workflow, id, &mut scan)?;
        }
    }
    Ok(scan)
}
