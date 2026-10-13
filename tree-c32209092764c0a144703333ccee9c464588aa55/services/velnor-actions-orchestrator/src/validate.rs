//! Staged-tree validation before any generate write.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::GeneratorLock;
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock, verify_version_policy};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ProcessOutput, ToolCatalog};
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::generate::write_tree;
use crate::validate_shell::{run_shellcheck_bodies, run_shellcheck_probe};
use crate::validate_zizmor::{run_zizmor, write_zizmor_config};

/// Velnor-repository-only bootstrap lock (never read for consumers).
const GENERATOR_LOCK_REL: &str = ".velnor/generator.lock";

/// Velnor-repository-only version-policy mirror (never read for consumers).
const VERSION_POLICY_REL: &str = ".velnor/version-policy.toml";

/// Staged actionlint config consumed via `-config-file`.
const ACTIONLINT_CONFIG: &str = ".github/actionlint.yaml";

/// Staged workflows directory prefix.
const WORKFLOWS_DIR: &str = ".github/workflows/";

/// Cap for validator diagnostics embedded in errors.
const DIAG_CAP: usize = 4000;

/// Validate the rendered tree in isolated staging; fail closed.
///
/// Runs before any replace or preview write, so any validator failure,
/// tool failure, or empty workflow set leaves all output untouched.
/// Returns the sorted pinned-validator specs that accepted the tree.
pub(crate) fn validate_staged(tree: &RenderedTree) -> Result<Vec<String>, OrchestratorError> {
    let staging = tempfile::tempdir().map_err(|err| {
        OrchestratorError::io(std::env::temp_dir().display().to_string(), err.to_string())
    })?;
    write_tree(&staging.path().join(".github"), tree)?;
    let workflows = staged_workflows(tree)?;
    let catalog = ToolCatalog::pinned();
    run_actionlint(&catalog, staging.path(), &workflows)?;
    run_shellcheck_probe(&catalog, staging.path())?;
    write_zizmor_config(staging.path(), tree)?;
    run_zizmor(&catalog, staging.path())?;
    run_shellcheck_bodies(&catalog, staging.path(), &workflows)?;
    let mut validated = vec![
        catalog.tool_spec(PinnedTool::Actionlint),
        catalog.tool_spec(PinnedTool::Shellcheck),
        catalog.tool_spec(PinnedTool::Zizmor),
    ];
    validated.sort();
    Ok(validated)
}

/// Verify Velnor-repository bootstrap files; return the lock when present.
///
/// Fails when the version-policy mirror differs from the compiled catalog
/// or when the lock is malformed. Missing files are skipped (pre-seed
/// trust-on-review); the plan job then carries no Acquire step. Consumer
/// generation never calls this: it must not read either file.
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

/// Map a lock failure onto the contract error channel.
#[expect(clippy::needless_pass_by_value, reason = "map_err passes owned errors")]
fn contract_of(err: impl ToString) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

/// Workflow files: under `.github/workflows/` with a YAML extension.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "rendered tree paths are exact lowercase by construction"
)]
pub(crate) fn is_workflow_path(path: &str) -> bool {
    path.starts_with(WORKFLOWS_DIR) && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

/// Staged workflow files, sorted; empty is a fail-closed error.
fn staged_workflows(tree: &RenderedTree) -> Result<Vec<String>, OrchestratorError> {
    if !tree.files.iter().any(|file| file.path == ACTIONLINT_CONFIG) {
        return Err(OrchestratorError::Validation {
            tool: "actionlint".to_owned(),
            problem: "missing_actionlint_config".to_owned(),
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
            tool: "actionlint".to_owned(),
            problem: "no_workflows_to_validate".to_owned(),
        });
    }
    Ok(workflows)
}

/// Staged actionlint argv from the actionlint toolchain (GEN-2.16).
fn actionlint_argv(workflows: &[String]) -> Vec<OsString> {
    velnor_actions_actionlint::ActionlintToolchain::staged_lint_argv(ACTIONLINT_CONFIG, workflows)
        .iter()
        .map(OsString::from)
        .collect()
}

/// Run pinned actionlint, with shellcheck on, over staged workflows.
fn run_actionlint(
    catalog: &ToolCatalog,
    staging: &Path,
    workflows: &[String],
) -> Result<(), OrchestratorError> {
    let args = actionlint_argv(workflows);
    let output = pinned_output(
        catalog,
        "actionlint",
        vec![PinnedTool::Actionlint, PinnedTool::Shellcheck],
        args,
        staging,
    )?;
    if output.success {
        Ok(())
    } else {
        Err(OrchestratorError::Validation {
            tool: "actionlint".to_owned(),
            problem: diagnose(&output),
        })
    }
}

/// Execute one pinned tool in staging; spawn failure fails closed.
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

/// Validator diagnostics, capped; exit code when streams are empty.
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
#[cfg(test)]
mod tests;
