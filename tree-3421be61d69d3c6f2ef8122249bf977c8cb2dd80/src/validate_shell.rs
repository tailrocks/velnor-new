//! Shell linting for staged `run:` bodies with pinned shellcheck.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::{PinnedTool, ProcessOutput, ToolCatalog};

use crate::OrchestratorError;
use crate::validate::{diagnose, pinned_output};

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

/// Lint every staged `run:` body with pinned shellcheck; fail closed.
pub(crate) fn run_shellcheck_bodies(
    catalog: &ToolCatalog,
    staging: &Path,
    workflows: &[String],
) -> Result<(), OrchestratorError> {
    for (index, body) in staged_run_bodies(staging, workflows)?.iter().enumerate() {
        if body.trim().is_empty() {
            continue;
        }
        let path = staging.join(format!("shellcheck-{index}.sh"));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n"))
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        let output = shellcheck_output(
            catalog,
            vec![
                OsString::from("-S"),
                OsString::from("warning"),
                OsString::from("--format=gcc"),
                path.into_os_string(),
            ],
            staging,
        )?;
        if !output.success {
            return Err(OrchestratorError::Validation {
                tool: "shellcheck".to_owned(),
                problem: diagnose(&output),
            });
        }
    }
    Ok(())
}

/// Extract `run:` bodies from staged workflows; block scalars fail closed.
fn staged_run_bodies(
    staging: &Path,
    workflows: &[String],
) -> Result<Vec<String>, OrchestratorError> {
    let mut bodies = Vec::new();
    for rel in workflows {
        let path = staging.join(rel);
        let text = std::fs::read_to_string(&path)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix("run:") else {
                continue;
            };
            let scalar = rest.strip_prefix(' ').unwrap_or(rest).trim_end();
            if scalar.is_empty() || scalar.starts_with(['|', '>']) {
                return Err(shellcheck_fail("run_block_scalar_unlintable"));
            }
            bodies.push(unquote_run_scalar(scalar)?);
        }
    }
    Ok(bodies)
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
mod tests {
    use super::*;

    /// One staged workflow carrying a single `run:` line, renderer-shaped.
    fn staged_run(run: &str) -> Result<(tempfile::TempDir, Vec<String>), String> {
        let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
        let rel = ".github/workflows/t.yml".to_owned();
        std::fs::create_dir_all(dir.path().join(".github/workflows"))
            .map_err(|err| err.to_string())?;
        std::fs::write(
            dir.path().join(&rel),
            format!("jobs:\n  a:\n    steps:\n      - name: t\n        run: {run}\n"),
        )
        .map_err(|err| err.to_string())?;
        Ok((dir, vec![rel]))
    }

    #[test]
    fn shellcheck_clean_passes_violation_and_block_fail() -> Result<(), String> {
        let catalog = ToolCatalog::pinned();
        let (clean, workflows) = staged_run("echo \"hi\"")?;
        run_shellcheck_bodies(&catalog, clean.path(), &workflows).map_err(|err| err.to_string())?;
        // SC2086 is info-level in shellcheck 0.11 (below `-S warning`), so the
        // violation pairs an unquoted var with error-level SC2070 to trip `-S warning`.
        let (dirty, workflows) = staged_run("echo $FOO/bar && [ -n $BAZ ]")?;
        assert!(
            run_shellcheck_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
                matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                    && err.to_string().contains("SC2070")
            })
        );
        let (blocked, workflows) = staged_run("|")?;
        assert!(
            run_shellcheck_bodies(&catalog, blocked.path(), &workflows)
                .is_err_and(|err| { err.to_string().contains("run_block_scalar_unlintable") })
        );
        Ok(())
    }

    #[test]
    fn shellcheck_version_mismatch_fails() {
        assert!(check_shellcheck_version("ShellCheck\nversion: 0.11.0\n", "0.11.0").is_ok());
        assert!(
            check_shellcheck_version("junk", "0.11.0")
                .is_err_and(|err| { err.to_string().contains("shellcheck_version_mismatch") })
        );
        assert!(
            check_shellcheck_version("version: 9.9.9\n", "0.11.0")
                .is_err_and(|err| { err.to_string().contains("shellcheck_version_mismatch") })
        );
    }
}
