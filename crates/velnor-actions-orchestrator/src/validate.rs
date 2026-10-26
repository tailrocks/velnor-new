// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::GeneratorLock;
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock, verify_version_policy};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ProcessOutput, ToolCatalog};
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::generate::write_tree;
use crate::validate_shell::{run_shellcheck_bodies, run_shellcheck_probe};

const GENERATOR_LOCK_REL: &str = ".velnor/generator.lock";

const VERSION_POLICY_REL: &str = ".velnor/version-policy.toml";

const LINT_CONFIG: &str = velnor_actions_actionlint::JACTIONLINT_CONFIG_PATH;

const WORKFLOWS_DIR: &str = ".github/workflows/";

const ACTIONS_DIR: &str = ".github/actions/";

const DIAG_CAP: usize = 4000;

pub(crate) fn validate_staged(tree: &RenderedTree) -> Result<Vec<String>, OrchestratorError> {
    let staging = tempfile::tempdir().map_err(|err| {
        OrchestratorError::io(std::env::temp_dir().display().to_string(), err.to_string())
    })?;
    write_tree(&staging.path().join(".github"), tree)?;
    let workflows = staged_workflows(tree)?;
    let catalog = ToolCatalog::pinned();
    run_jactionlint(
        &catalog,
        staging.path(),
        &staged_lint_files(tree, &workflows),
    )?;
    run_shellcheck_probe(&catalog, staging.path())?;
    run_shellcheck_bodies(&catalog, staging.path(), &workflows)?;
    let mut validated = vec![
        catalog.tool_spec(PinnedTool::Jactionlint),
        catalog.tool_spec(PinnedTool::Shellcheck),
    ];
    validated.sort();
    Ok(validated)
}

pub(crate) fn verify_velnor_repository_files(
    root: &Path,
) -> Result<Option<GeneratorLock>, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let policy = root.join(VERSION_POLICY_REL);
    if policy.is_file() {
        let text = load_text(&policy).map_err(contract_of)?;
        verify_version_policy(&text, &catalog).map_err(contract_of)?;
    }
    let lock_path = root.join(GENERATOR_LOCK_REL);
    if !lock_path.is_file() {
        return Ok(None);
    }
    let text = load_text(&lock_path).map_err(contract_of)?;
    parse_generator_lock(&text).map(Some).map_err(contract_of)
}

#[expect(clippy::needless_pass_by_value, reason = "map_err passes owned errors")]
fn contract_of(err: impl ToString) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "rendered tree paths are exact lowercase by construction"
)]
pub(crate) fn is_workflow_path(path: &str) -> bool {
    path.starts_with(WORKFLOWS_DIR) && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

pub(crate) fn is_action_path(path: &str) -> bool {
    path.starts_with(ACTIONS_DIR)
        && (path.ends_with("/action.yml") || path.ends_with("/action.yaml"))
}

fn staged_lint_files(tree: &RenderedTree, workflows: &[String]) -> Vec<String> {
    let mut actions: Vec<String> = tree
        .files
        .iter()
        .map(|file| file.path.clone())
        .filter(|path| is_action_path(path))
        .collect();
    actions.sort();
    workflows.iter().cloned().chain(actions).collect()
}

fn staged_workflows(tree: &RenderedTree) -> Result<Vec<String>, OrchestratorError> {
    if !tree.files.iter().any(|file| file.path == LINT_CONFIG) {
        return Err(OrchestratorError::Validation {
            tool: "jactionlint".to_owned(),
            problem: "missing_jactionlint_config".to_owned(),
        });
    }
    let mut workflows: Vec<String> = tree
        .files
        .iter()
        .map(|file| file.path.clone())
        .filter(|path| is_workflow_path(path))
        .collect();
    workflows.sort();
    if workflows.is_empty() {
        return Err(OrchestratorError::Validation {
            tool: "jactionlint".to_owned(),
            problem: "no_workflows_to_validate".to_owned(),
        });
    }
    Ok(workflows)
}

fn jactionlint_argv(files: &[String]) -> Vec<OsString> {
    velnor_actions_actionlint::JactionlintToolchain::staged_lint_argv(LINT_CONFIG, files)
        .iter()
        .map(OsString::from)
        .collect()
}

fn run_jactionlint(
    catalog: &ToolCatalog,
    staging: &Path,
    files: &[String],
) -> Result<(), OrchestratorError> {
    let output = pinned_output(
        catalog,
        "jactionlint",
        vec![PinnedTool::Jactionlint, PinnedTool::Shellcheck],
        jactionlint_argv(files),
        staging,
    )?;
    if output.success {
        Ok(())
    } else {
        Err(OrchestratorError::Validation {
            tool: "jactionlint".to_owned(),
            problem: diagnose(&output),
        })
    }
}

pub(crate) fn pinned_output(
    catalog: &ToolCatalog,
    tool: &str,
    tools: Vec<PinnedTool>,
    args: Vec<OsString>,
    staging: &Path,
) -> Result<ProcessOutput, OrchestratorError> {
    let program = OsString::from(tool);
    let exec = PinnedToolExec::new(tools, &program, args).map_err(|err| {
        OrchestratorError::Validation {
            tool: tool.to_owned(),
            problem: err.to_string(),
        }
    })?;
    let command = exec
        .command(catalog)
        .map_err(|err| OrchestratorError::Validation {
            tool: tool.to_owned(),
            problem: err.to_string(),
        })?;
    command
        .with_cwd(staging.to_path_buf())
        .run()
        .map_err(|err| OrchestratorError::Validation {
            tool: tool.to_owned(),
            problem: err.to_string(),
        })
}

pub(crate) fn diagnose(output: &ProcessOutput) -> String {
    let bytes = if output.stdout.is_empty() {
        output.stderr.as_slice()
    } else {
        output.stdout.as_slice()
    };
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if text.len() > DIAG_CAP {
        text.truncate(DIAG_CAP);
        text.push_str("…[truncated]");
    }
    if text.trim().is_empty() {
        let code = output
            .code
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        return format!("exit_code:{code}");
    }
    text
}
