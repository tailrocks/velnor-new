// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use super::waste_oracle::{children, installs_toolchain, run_scalar};

type Env = BTreeMap<String, String>;

#[derive(Debug)]
pub(super) struct StepText {
    pub(super) name: String,
    pub(super) uses: Option<String>,
    pub(super) run: Option<String>,
    env: Env,
    installs_rust: bool,
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
