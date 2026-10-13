//! Shell linting for staged `run:` bodies with pinned shellcheck.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::{PinnedTool, ProcessOutput, ToolCatalog};

use crate::validate::{diagnose, pinned_output};
use velnor_actions_orchestrator_core::OrchestratorError;

mod workflow;

/// Prove the pinned shellcheck resolves, runs, and reports its pin.
pub(crate) fn run_shellcheck_probe(
    catalog: &ToolCatalog,
    staging: &Path,
) -> Result<(), OrchestratorError> {
    let output = shellcheck_output(catalog, vec![OsString::from("--version")], staging)?;
    if !output.success {
        return Err(OrchestratorError::Validation {
            tool: "shellcheck".to_owned(),
            problem: diagnose(&output),
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    check_shellcheck_version(&text, catalog.version(PinnedTool::Shellcheck))
}

/// Run the pinned shellcheck binary with fixed args in staging.
fn shellcheck_output(
    catalog: &ToolCatalog,
    args: Vec<OsString>,
    staging: &Path,
) -> Result<ProcessOutput, OrchestratorError> {
    pinned_output(
        catalog,
        "shellcheck",
        vec![PinnedTool::Shellcheck],
        args,
        staging,
    )
}

/// Fail when `--version` output lacks the pinned `version:` line.
fn check_shellcheck_version(output: &str, pinned: &str) -> Result<(), OrchestratorError> {
    let found = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("version:"))
        .map(str::trim);
    if found == Some(pinned) {
        Ok(())
    } else {
        Err(shellcheck_fail(&format!(
            "shellcheck_version_mismatch:found_{}_pinned_{pinned}",
            found.unwrap_or("unparseable")
        )))
    }
}

/// Lint every staged `run:` body with one pinned shellcheck run; fail closed.
///
/// Bodies ride one argv (shellcheck checks each file and reports per-file
/// gcc diagnostics), so per-crate jobs add files, not subprocesses.
pub(crate) fn run_shellcheck_bodies(
    catalog: &ToolCatalog,
    staging: &Path,
    workflows: &[String],
) -> Result<(), OrchestratorError> {
    let bodies = workflow::staged_runs(staging, workflows)?;
    let mut files: Vec<OsString> = Vec::new();
    for (index, run) in bodies.iter().enumerate() {
        if run.body.trim().is_empty() {
            continue;
        }
        let path = staging.join(format!("shellcheck-{index}.sh"));
        std::fs::write(&path, format!("{}\n{}\n", run.shell.shebang(), run.body))
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        files.push(path.into_os_string());
    }
    if files.is_empty() {
        return Ok(());
    }
    // `-S warning` pass. SC2086 (unquoted expansion) is info-level in
    // shellcheck 0.11, so it sits below this floor; `--enable` cannot
    // promote it (it only switches on default-off optional checks).
    let mut args = vec![
        OsString::from("-S"),
        OsString::from("warning"),
        OsString::from("--format=gcc"),
    ];
    args.extend(files.iter().cloned());
    let output = shellcheck_output(catalog, args, staging)?;
    if !output.success {
        return Err(OrchestratorError::Validation {
            tool: "shellcheck".to_owned(),
            problem: diagnose(&output),
        });
    }
    // Targeted SC2086 pass: `--include` restricts output to that code, so
    // with no severity floor the exit status reflects SC2086 alone.
    let mut args = vec![
        OsString::from("--format=gcc"),
        OsString::from("--include=SC2086"),
    ];
    args.extend(files);
    let output = shellcheck_output(catalog, args, staging)?;
    if output.success {
        Ok(())
    } else {
        Err(OrchestratorError::Validation {
            tool: "shellcheck".to_owned(),
            problem: diagnose(&output),
        })
    }
}

/// Unquote one single-line scalar: plain or double-quoted only.
fn unquote_run_scalar(scalar: &str) -> Result<String, OrchestratorError> {
    let scalar = scalar.trim();
    if !scalar.starts_with('"') {
        if scalar.starts_with('\'') {
            return Err(shellcheck_fail("run_single_quoted_unlintable"));
        }
        return Ok(scalar.to_owned());
    }
    let Some(body) = scalar
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Err(shellcheck_fail("run_scalar_unbalanced"));
    };
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(escaped @ ('\\' | '"')) => out.push(escaped),
            _ => return Err(shellcheck_fail("run_scalar_bad_escape")),
        }
    }
    Ok(out)
}

/// Shorthand for a shellcheck validation failure.
fn shellcheck_fail(problem: &str) -> OrchestratorError {
    OrchestratorError::Validation {
        tool: "shellcheck".to_owned(),
        problem: problem.to_owned(),
    }
}
#[cfg(test)]
mod tests;
