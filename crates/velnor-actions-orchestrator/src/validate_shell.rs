//! Shell linting for staged workflow bodies and generated shared scripts.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::is_generated_marker_line;
use velnor_actions_mise::{PinnedTool, ProcessOutput, ToolCatalog};
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::validate::{diagnose, pinned_output};

#[path = "validate_shell_yaml.rs"]
mod workflow;

const SHARED_SCRIPT_PREFIX: &str = ".github/scripts/velnor-shared/";

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

/// Lint staged `run:` bodies and generated shared scripts with pinned shellcheck.
///
/// Bodies ride one argv (shellcheck checks each file and reports per-file
/// gcc diagnostics), so per-crate jobs add files, not subprocesses.
pub(crate) fn run_shellcheck_bodies(
    catalog: &ToolCatalog,
    staging: &Path,
    workflows: &[String],
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    let mut bodies = workflow::staged_runs(staging, workflows)?;
    bodies.extend(staged_shared_scripts(tree)?);
    let mut files: Vec<OsString> = Vec::new();
    for (index, run) in bodies.iter().enumerate() {
        if run.body.trim().is_empty() {
            continue;
        }
        let extension = match run.shell {
            workflow::ShellDialect::Bash => "bash",
            workflow::ShellDialect::Sh => "sh",
        };
        let path = staging.join(format!("shellcheck-{index}.{extension}"));
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

/// Select exact content-addressed shared shell assets from the rendered tree.
fn staged_shared_scripts(
    tree: &RenderedTree,
) -> Result<Vec<workflow::StagedRun>, OrchestratorError> {
    let mut runs = Vec::new();
    for file in &tree.files {
        if !file.path.starts_with(SHARED_SCRIPT_PREFIX) {
            continue;
        }
        let shell = shared_script_dialect(&file.path)
            .ok_or_else(|| shellcheck_fail("shared_script_path_invalid"))?;
        let Some((marker, body)) = file.bytes.split_once('\n') else {
            return Err(shellcheck_fail("shared_script_marker_missing"));
        };
        if !is_generated_marker_line(marker) {
            return Err(shellcheck_fail("shared_script_marker_missing"));
        }
        runs.push(workflow::StagedRun {
            body: body.to_owned(),
            shell,
        });
    }
    Ok(runs)
}

/// Match only the renderer's two content-addressed shared-script path forms.
fn shared_script_dialect(path: &str) -> Option<workflow::ShellDialect> {
    let filename = path.strip_prefix(SHARED_SCRIPT_PREFIX)?;
    let (digest, shell) = if let Some(digest) = filename
        .strip_prefix("sh-b3-")
        .and_then(|rest| rest.strip_suffix(".sh"))
    {
        (digest, workflow::ShellDialect::Sh)
    } else {
        let digest = filename
            .strip_prefix("bash-b3-")
            .and_then(|rest| rest.strip_suffix(".bash"))?;
        (digest, workflow::ShellDialect::Bash)
    };
    let valid_digest = digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    valid_digest.then_some(shell)
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

    fn lint_workflow_bodies(
        catalog: &ToolCatalog,
        staging: &Path,
        workflows: &[String],
    ) -> Result<(), OrchestratorError> {
        run_shellcheck_bodies(
            catalog,
            staging,
            workflows,
            &RenderedTree {
                files: Vec::new(),
                symlinks: Vec::new(),
            },
        )
    }

    /// One staged workflow carrying a single `run:` line, renderer-shaped.
    fn staged_run(run: &str) -> Result<(tempfile::TempDir, Vec<String>), String> {
        staged_runs(std::slice::from_ref(&run))
    }

    /// One staged workflow carrying one `run:` line per entry, in order.
    fn staged_runs(runs: &[&str]) -> Result<(tempfile::TempDir, Vec<String>), String> {
        let mut yaml = String::from("jobs:\n  a:\n    runs-on: ubuntu-26.04\n    steps:\n");
        for (index, run) in runs.iter().enumerate() {
            use std::fmt::Write as _;
            writeln!(yaml, "      - name: t{index}\n        run: {run}")
                .map_err(|err| err.to_string())?;
        }
        staged_workflow(&yaml)
    }

    fn staged_workflow(yaml: &str) -> Result<(tempfile::TempDir, Vec<String>), String> {
        let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
        let rel = ".github/workflows/t.yml".to_owned();
        std::fs::create_dir_all(dir.path().join(".github/workflows"))
            .map_err(|err| err.to_string())?;
        std::fs::write(dir.path().join(&rel), yaml).map_err(|err| err.to_string())?;
        Ok((dir, vec![rel]))
    }

    #[test]
    fn shellcheck_clean_passes_violation_and_block_fail() -> Result<(), String> {
        let catalog = ToolCatalog::pinned();
        let (clean, workflows) = staged_run("echo \"hi\"")?;
        lint_workflow_bodies(&catalog, clean.path(), &workflows).map_err(|err| err.to_string())?;
        // Error-level SC2070 trips the `-S warning` pass (see
        // `shellcheck_pure_sc2086_body_fails` for the targeted SC2086 pass).
        let (dirty, workflows) = staged_run("echo $FOO/bar && [ -n $BAZ ]")?;
        assert!(
            lint_workflow_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
                matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                    && err.to_string().contains("SC2070")
            })
        );
        let (blocked, workflows) = staged_run("|")?;
        assert!(
            lint_workflow_bodies(&catalog, blocked.path(), &workflows)
                .is_err_and(|err| { err.to_string().contains("run_block_scalar_unlintable") })
        );
        Ok(())
    }

    #[test]
    fn shellcheck_pure_sc2086_body_fails() -> Result<(), String> {
        // SC2086 is info-level in shellcheck 0.11, so a pure-SC2086 body
        // passes `-S warning` and only the targeted `--include=SC2086`
        // pass fails it: deleting that pass turns this test green-on-red.
        let catalog = ToolCatalog::pinned();
        let (dirty, workflows) = staged_run("echo $FOO/bar")?;
        assert!(
            lint_workflow_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
                matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                    && err.to_string().contains("SC2086")
            })
        );
        Ok(())
    }

    #[test]
    fn shellcheck_batch_lints_every_body() -> Result<(), String> {
        let catalog = ToolCatalog::pinned();
        let (clean, workflows) = staged_runs(&["echo \"one\"", "echo \"two\""])?;
        lint_workflow_bodies(&catalog, clean.path(), &workflows).map_err(|err| err.to_string())?;
        // A violation in a later body still fails the single batched run.
        let (dirty, workflows) = staged_runs(&["echo \"one\"", "echo $FOO/bar && [ -n $BAZ ]"])?;
        assert!(
            lint_workflow_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
                matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                    && err.to_string().contains("SC2070")
            })
        );
        // All-empty bodies lint nothing and pass.
        let (empty, workflows) = staged_runs(&["\"\""])?;
        lint_workflow_bodies(&catalog, empty.path(), &workflows).map_err(|err| err.to_string())?;
        Ok(())
    }

    #[test]
    fn shellcheck_uses_the_resolved_bash_or_posix_dialect() -> Result<(), String> {
        let catalog = ToolCatalog::pinned();
        let yaml = |shell: &str| {
            format!(
                "jobs:\n  probe:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: {shell}\n    steps:\n      - name: array\n        run: \"items=(one); test \\\"${{items[0]}}\\\" = one\"\n"
            )
        };
        let (bash, workflows) = staged_workflow(&yaml("bash -e {0}"))?;
        lint_workflow_bodies(&catalog, bash.path(), &workflows).map_err(|err| err.to_string())?;
        let (sh, workflows) = staged_workflow(&yaml("sh -e {0}"))?;
        assert!(lint_workflow_bodies(&catalog, sh.path(), &workflows).is_err());

        let container_yaml = "jobs:\n  scale-container:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: posix\n        run: test -n \"$HOME\" && printf '%s\\n' ready\n    container: alpine:3.22\n";
        let (container, workflows) = staged_workflow(container_yaml)?;
        lint_workflow_bodies(&catalog, container.path(), &workflows)
            .map_err(|err| err.to_string())?;
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

#[cfg(test)]
#[path = "validate_shell_shared_tests.rs"]
mod shared_script_tests;
