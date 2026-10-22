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

#[cfg(test)]
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{render::RenderedFile, yaml::quote_scalar};

use crate::OrchestratorError;
use crate::verifier_actions::{VerifierAction, VerifierActionSet};

/// Text that installs or changes a Rust toolchain.
const INSTALLS: [&str; 9] = [
    " install 'rust@",
    " install rust@",
    " install \"rust@",
    " install \\\"rust@",
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

/// A shell step that installs Rust and verifies it inside the same script
/// (`upgrades.yml`) calls this between the install and any use.
const SELF_VERIFY: &str = "$VELNOR_VERIFY_RUST_SCRIPT";

/// One step: its name and full text.
#[derive(Clone)]
struct StepText {
    name: String,
    text: String,
    fields: Vec<(String, String)>,
    top_fields: Vec<(String, String)>,
    env_fields: Vec<(String, String)>,
    unexpected_nested: bool,
    run_scalar: Option<String>,
}

/// Fail generation when a rendered file installs Rust without verifying it.
///
/// # Errors
///
/// Returns a contract error `rust_install_unverified:<path>:<step>`.
#[cfg(test)]
pub(crate) fn audit(files: &[RenderedFile]) -> Result<(), OrchestratorError> {
    let verifiers = VerifierActionSet::build(
        &ToolCatalog::pinned().for_emission(),
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: format!("verifier_action_templates_invalid:{error}"),
    })?;
    audit_with_verifiers(files, &verifiers)
}

/// Validate all rendered Rust installs against exact verifier call/file
/// templates generated from the same catalog as the workflow IR.
///
/// # Errors
///
/// Returns a contract error for an install without its canonical check or an
/// altered/missing verifier action definition or call.
pub(crate) fn audit_with_verifiers(
    files: &[RenderedFile],
    verifiers: &VerifierActionSet,
) -> Result<(), OrchestratorError> {
    check_verifier_files(files, verifiers)?;
    for file in files {
        let yaml = std::path::Path::new(&file.path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"));
        if !(file.path.starts_with(".github/") && yaml) {
            continue;
        }
        for (owner, steps) in step_lists(&file.bytes) {
            for step in &steps {
                if !is_verifier_step_valid(step, verifiers) {
                    return Err(unverified(
                        &format!("{}:{owner}", file.path),
                        &step.name,
                        "noncanonical verifier call or body",
                    ));
                }
            }
            check_steps(&format!("{}:{owner}", file.path), &steps, verifiers)?;
        }
    }
    Ok(())
}

fn check_verifier_files(
    files: &[RenderedFile],
    verifiers: &VerifierActionSet,
) -> Result<(), OrchestratorError> {
    let expected = verifiers.files();
    let expected_paths: std::collections::BTreeSet<&str> =
        expected.iter().map(|file| file.path.as_str()).collect();
    for action in [
        &verifiers.restored_rust,
        &verifiers.installed_rust,
        &verifiers.jactionlint,
    ] {
        let matching: Vec<_> = files
            .iter()
            .filter(|file| file.path == action.file.path)
            .collect();
        if matching.len() != 1 {
            return Err(unverified(
                &action.file.path,
                action.name,
                "missing or duplicate verifier action definition",
            ));
        }
        if matching[0].bytes != action.file.bytes {
            return Err(unverified(
                &action.file.path,
                action.name,
                "verifier action body, shell, environment, or marker changed",
            ));
        }
    }
    for file in files.iter().filter(|file| {
        file.path
            .strip_prefix(".github/actions/velnor-verify-")
            .is_some_and(|_| file.path.ends_with("/action.yml"))
    }) {
        if !expected_paths.contains(file.path.as_str()) {
            return Err(unverified(
                &file.path,
                "unregistered verifier action",
                "unexpected generated verifier definition",
            ));
        }
    }
    Ok(())
}

fn is_verifier_step_valid(step: &StepText, verifiers: &VerifierActionSet) -> bool {
    if let Some(uses) = step.uses_value()
        && let Some(action) = verifiers.by_uses(uses)
    {
        return canonical_action_call(step, action);
    }
    if let Some(templates) = verifiers.inline_rust_templates.get(step.name.as_str()) {
        let matches = templates
            .iter()
            .any(|template| canonical_inline_shape(step, step.name.as_str(), template));
        return matches;
    }
    if step.name == verifiers.jactionlint.name {
        return verifiers
            .inline_jactionlint_templates
            .iter()
            .any(|template| canonical_inline_shape(step, verifiers.jactionlint.name, template));
    }
    true
}

fn canonical_inline_shape(
    step: &StepText,
    name: &str,
    template: &crate::verifier_actions::InlineVerifierTemplate,
) -> bool {
    if step.unexpected_nested
        || template
            .required_env
            .iter()
            .any(|key| !template.env.contains_key(*key))
    {
        return false;
    }
    let Some(shell) = template.shell else {
        return false;
    };
    let expected_top = vec![
        ("name".to_owned(), quote_scalar(name)),
        ("env".to_owned(), String::new()),
        ("shell".to_owned(), quote_scalar(shell)),
        ("run".to_owned(), template.run_scalar.clone()),
    ];
    let mut actual_env = step.env_fields.clone();
    actual_env.sort_unstable();
    let expected_env = template
        .env
        .iter()
        .map(|(key, value)| (key.clone(), quote_scalar(value)))
        .collect::<Vec<_>>();
    step.top_fields == expected_top && actual_env == expected_env
}

fn canonical_action_call(step: &StepText, action: &VerifierAction) -> bool {
    step.name == action.name
        && step.fields.len() == 2
        && step.fields[0].0 == "name"
        && step.fields[0].1 == action.name
        && step.fields[1].0 == "uses"
        && step.fields[1].1 == format!("{} # zizmor: ignore[self-repository]", action.uses)
}

fn unverified(path: &str, step: &str, why: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("rust_install_unverified:{path}:{step}:{why}"),
    }
}

fn check_steps(
    path: &str,
    steps: &[StepText],
    verifiers: &VerifierActionSet,
) -> Result<(), OrchestratorError> {
    let mut index = 0;
    while index < steps.len() {
        if !installs_rust(&steps[index]) {
            index += 1;
            continue;
        }
        if self_verified(&steps[index]) {
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
            if is_rust_verifier(step, verifiers) {
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

/// The step verifies its own install: the check runs after every install in
/// its text and before every use of Rust, Cargo or MBX.
fn self_verified(step: &StepText) -> bool {
    let text = step.run_scalar.as_deref().unwrap_or(&step.text);
    let Some(check) = text.find(SELF_VERIFY) else {
        return false;
    };
    let last_install = INSTALLS
        .iter()
        .filter_map(|needle| text.rfind(needle))
        .max()
        .unwrap_or(0);
    let first_use = USES
        .iter()
        .filter_map(|needle| text.find(needle))
        .min()
        .unwrap_or(usize::MAX);
    last_install < check && check < first_use
}

fn installs_rust(step: &StepText) -> bool {
    INSTALLS.iter().any(|needle| step.text.contains(needle))
}

fn is_rust_verifier(step: &StepText, verifiers: &VerifierActionSet) -> bool {
    if step.uses_value() == Some(verifiers.installed_rust.uses) {
        return canonical_action_call(step, &verifiers.installed_rust);
    }
    verifiers
        .inline_rust_templates
        .get(step.name.as_str())
        .is_some_and(|templates| {
            templates
                .iter()
                .any(|template| canonical_inline_shape(step, &step.name, template))
        })
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Every `steps:` sequence of a rendered file with its owning job (or
/// `runs` for a composite action), as step texts in order.
fn step_lists(text: &str) -> Vec<(String, Vec<StepText>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut anchors: BTreeMap<String, StepText> = BTreeMap::new();
    let mut run_anchors: BTreeMap<String, String> = BTreeMap::new();
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
                steps.push(step_text(&block, &mut anchors, &mut run_anchors));
            }
            lists.push((owner, steps));
            continue;
        }
        at += 1;
    }
    lists
}

/// One step block; `- &sN` records an anchor and `- *sN` resolves one.
fn step_text(
    block: &[&str],
    anchors: &mut BTreeMap<String, StepText>,
    run_anchors: &mut BTreeMap<String, String>,
) -> StepText {
    let head = block[0].trim_start().trim_start_matches("- ").trim();
    if let Some(alias) = head.strip_prefix('*') {
        return anchors.get(alias).map_or_else(
            || StepText {
                name: format!("*{alias}"),
                text: String::new(),
                fields: Vec::new(),
                top_fields: Vec::new(),
                env_fields: Vec::new(),
                unexpected_nested: true,
                run_scalar: None,
            },
            |step| StepText {
                name: step.name.clone(),
                text: step.text.clone(),
                fields: step.fields.clone(),
                top_fields: step.top_fields.clone(),
                env_fields: step.env_fields.clone(),
                unexpected_nested: step.unexpected_nested,
                run_scalar: step.run_scalar.clone(),
            },
        );
    }
    let fields: Vec<(String, String)> = block
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_start().trim_start_matches("- ").trim();
            let (key, value) = trimmed.split_once(": ")?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect();
    let item_indent = indent(block[0]);
    let mut top_fields = Vec::new();
    let mut env_fields = Vec::new();
    let mut in_env = false;
    let mut unexpected_nested = false;
    for (index, line) in block.iter().enumerate() {
        let trimmed = line.trim_start().trim_start_matches("- ").trim();
        let field = trimmed
            .split_once(": ")
            .or_else(|| trimmed.strip_suffix(':').map(|key| (key, "")));
        let Some((key, value)) = field else {
            if index > 0 && !line.trim().is_empty() {
                unexpected_nested = true;
            }
            continue;
        };
        let line_indent = indent(line);
        if index == 0 || line_indent == item_indent + 2 {
            top_fields.push((key.to_owned(), value.to_owned()));
            in_env = key == "env" && value.is_empty();
        } else if in_env && line_indent == item_indent + 4 {
            env_fields.push((key.to_owned(), value.to_owned()));
        } else {
            unexpected_nested = true;
        }
    }
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
    let run_scalar = block.iter().find_map(|line| {
        let trimmed = line.trim_start().trim_start_matches("- ").trim();
        let raw = trimmed.strip_prefix("run: ")?;
        if let Some(anchor) = raw.strip_prefix('&') {
            let (name, value) = anchor.split_once(' ')?;
            run_anchors.insert(name.to_owned(), value.to_owned());
            Some(value.to_owned())
        } else if let Some(alias) = raw.strip_prefix('*') {
            run_anchors.get(alias).cloned()
        } else {
            Some(raw.to_owned())
        }
    });
    let mut text = block.join("\n");
    if let Some(run_scalar) = &run_scalar {
        text.push('\n');
        text.push_str(run_scalar);
    }
    let step = StepText {
        name,
        text,
        fields,
        top_fields,
        env_fields,
        unexpected_nested,
        run_scalar,
    };
    if let Some(anchor) = head.strip_prefix('&') {
        anchors.insert(
            anchor
                .split_whitespace()
                .next()
                .unwrap_or(anchor)
                .to_owned(),
            step.clone(),
        );
    }
    step
}

impl StepText {
    fn uses_value(&self) -> Option<&str> {
        self.fields.iter().find_map(|(key, value)| {
            (key == "uses").then(|| {
                value
                    .strip_suffix(" # zizmor: ignore[self-repository]")
                    .unwrap_or(value)
            })
        })
    }
}

#[cfg(test)]
#[path = "rust_install_audit_tests.rs"]
mod tests;
