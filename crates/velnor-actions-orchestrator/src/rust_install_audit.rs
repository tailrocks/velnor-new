//! Generate-time coverage audit: every generated workflow and composite
//! action that installs or changes a Rust toolchain verifies it before use.
//!
//! The lock audit reads only the `ci.yml` IR; release, qualification,
//! freshness, upgrade and composite-action files are rendered directly. This
//! audit reads every rendered `.github/**/*.yml` text instead, so no
//! renderer can add a Rust install that skips the check. For each `steps:`
//! list (YAML step anchors resolved) every step that installs or changes
//! Rust must be followed by [`VERIFY_RUST_TOOLCHAIN_STEP`] before any step
//! runs Rust, Cargo or MBX, and no install step may already run them.

use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::VERIFY_RUST_TOOLCHAIN_STEP;
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::OrchestratorError;

/// Text that installs or changes a Rust toolchain.
const INSTALLS: [&str; 7] = [
    " install 'rust@",
    " install rust@",
    "rustup toolchain install",
    "rustup component add",
    "rustup target add",
    "rustup-init",
    "rustup toolchain link",
];

/// Text of a step that runs Rust, Cargo or MBX.
const USES: [&str; 6] = [
    "exec 'rust@",
    "exec rust@",
    "-- rustc",
    "-- cargo",
    "-- mbx",
    "rustc --print",
];

/// Inside an install step: it already runs what it installs (`rustup`
/// itself, for components and targets, is allowed).
const RUNS: [&str; 4] = ["-- rustc", "-- cargo", "-- mbx", "rustc --print"];

/// One step: its name and full text.
struct StepText {
    name: String,
    text: String,
}

/// Fail generation when a rendered file installs Rust without verifying it.
///
/// # Errors
///
/// Returns a contract error `rust_install_unverified:<path>:<step>`.
pub(crate) fn audit(files: &[RenderedFile]) -> Result<(), OrchestratorError> {
    for file in files {
        let yaml = std::path::Path::new(&file.path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"));
        if !(file.path.starts_with(".github/") && yaml) {
            continue;
        }
        for (owner, steps) in step_lists(&file.bytes) {
            check_steps(&format!("{}:{owner}", file.path), &steps)?;
        }
    }
    Ok(())
}

fn unverified(path: &str, step: &str, why: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("rust_install_unverified:{path}:{step}:{why}"),
    }
}

fn check_steps(path: &str, steps: &[StepText]) -> Result<(), OrchestratorError> {
    let mut index = 0;
    while index < steps.len() {
        if !installs_rust(&steps[index]) {
            index += 1;
            continue;
        }
        let start = index;
        // The install run, then steps that never touch Rust (artifact
        // downloads, the Velnor helper), then the verification.
        loop {
            let Some(step) = steps.get(index) else {
                return Err(unverified(
                    path,
                    &steps[start].name,
                    "no verification follows the install",
                ));
            };
            if step.name == VERIFY_RUST_TOOLCHAIN_STEP {
                break;
            }
            if USES.iter().any(|needle| step.text.contains(needle)) && !installs_rust(step)
                || installs_rust(step) && RUNS.iter().any(|needle| step.text.contains(needle))
            {
                return Err(unverified(
                    path,
                    &step.name,
                    "runs the toolchain it installs before the verification",
                ));
            }
            index += 1;
        }
        index += 1;
    }
    Ok(())
}

fn installs_rust(step: &StepText) -> bool {
    step.name != VERIFY_RUST_TOOLCHAIN_STEP
        && INSTALLS.iter().any(|needle| step.text.contains(needle))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Every `steps:` sequence of a rendered file with its owning job (or
/// `runs` for a composite action), as step texts in order.
fn step_lists(text: &str) -> Vec<(String, Vec<StepText>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut anchors: BTreeMap<String, StepText> = BTreeMap::new();
    let mut lists = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim_end().ends_with("steps:") && line.trim() == "steps:" {
            let base = indent(line);
            let owner = lines[..at]
                .iter()
                .rev()
                .find(|candidate| indent(candidate) < base && candidate.trim_end().ends_with(':'))
                .map_or("", |candidate| candidate.trim().trim_end_matches(':'))
                .to_owned();
            let mut steps = Vec::new();
            at += 1;
            while at < lines.len() {
                let item = lines[at];
                if item.trim().is_empty() {
                    at += 1;
                    continue;
                }
                if indent(item) <= base {
                    break;
                }
                if !item.trim_start().starts_with("- ") {
                    at += 1;
                    continue;
                }
                let item_indent = indent(item);
                let mut block = vec![item];
                at += 1;
                while at < lines.len()
                    && (lines[at].trim().is_empty() || indent(lines[at]) > item_indent)
                {
                    block.push(lines[at]);
                    at += 1;
                }
                steps.push(step_text(&block, &mut anchors));
            }
            lists.push((owner, steps));
            continue;
        }
        at += 1;
    }
    lists
}

/// One step block; `- &sN` records an anchor and `- *sN` resolves one.
fn step_text(block: &[&str], anchors: &mut BTreeMap<String, StepText>) -> StepText {
    let head = block[0].trim_start().trim_start_matches("- ").trim();
    if let Some(alias) = head.strip_prefix('*') {
        return anchors.get(alias).map_or_else(
            || StepText {
                name: format!("*{alias}"),
                text: String::new(),
            },
            |step| StepText {
                name: step.name.clone(),
                text: step.text.clone(),
            },
        );
    }
    let text = block.join("\n");
    let name = block
        .iter()
        .find_map(|line| {
            let trimmed = line.trim_start().trim_start_matches("- ");
            trimmed.strip_prefix("name: ")
        })
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_owned();
    let step = StepText { name, text };
    if let Some(anchor) = head.strip_prefix('&') {
        anchors.insert(
            anchor
                .split_whitespace()
                .next()
                .unwrap_or(anchor)
                .to_owned(),
            StepText {
                name: step.name.clone(),
                text: step.text.clone(),
            },
        );
    }
    step
}

#[cfg(test)]
#[path = "rust_install_audit_tests.rs"]
mod tests;
