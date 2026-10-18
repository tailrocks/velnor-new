use std::path::Path;

use crate::OrchestratorError;

use super::{shellcheck_fail, unquote_run_scalar};

#[path = "validate_shell_yaml_shell.rs"]
mod shell;

#[path = "validate_shell_yaml_run_anchor.rs"]
mod run_anchor;

#[path = "validate_shell_yaml_alias_scope.rs"]
mod alias_scope;

#[path = "validate_shell_yaml_scan.rs"]
mod scan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellDialect {
    Bash,
    Sh,
}

impl ShellDialect {
    pub(super) const fn shebang(self) -> &'static str {
        match self {
            Self::Bash => "#!/bin/bash",
            Self::Sh => "#!/bin/sh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StagedRun {
    pub(super) body: String,
    pub(super) shell: ShellDialect,
}

#[derive(Default)]
struct JobScan {
    section: JobSection,
    runs_on: Option<String>,
    has_container: bool,
    default_shell: Option<ShellDialect>,
    current_step: Option<StepScan>,
    runs: Vec<StepScan>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum JobSection {
    #[default]
    Root,
    Defaults,
    DefaultsRun,
    Steps,
}

#[derive(Clone, Default)]
struct StepScan {
    body: Option<String>,
    shell: Option<ShellDialect>,
    nested_mapping: Option<String>,
    anchor: Option<String>,
    first_mapping_field_seen: bool,
}

/// Extract only `jobs.*.steps[*].run` from the renderer's emitted YAML grammar.
///
/// The workflow emitter uses fixed two-space indentation. This scanner follows
/// those exact parents instead of treating every YAML key named `run` as code.
pub(super) fn staged_runs(
    staging: &Path,
    workflows: &[String],
) -> Result<Vec<StagedRun>, OrchestratorError> {
    let mut runs = Vec::new();
    for rel in workflows {
        let path = staging.join(rel);
        let text = std::fs::read_to_string(&path)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        runs.extend(scan_workflow(&text)?);
    }
    Ok(runs)
}

fn scan_workflow(text: &str) -> Result<Vec<StagedRun>, OrchestratorError> {
    scan::scan_workflow(text)
}

fn scan_job_line(
    job: &mut JobScan,
    indent: usize,
    key: &str,
    value: &str,
) -> Result<(), OrchestratorError> {
    if indent == 4 {
        job.section = match (key, value) {
            ("defaults", "") => JobSection::Defaults,
            ("steps", "") => JobSection::Steps,
            _ => JobSection::Root,
        };
        if key == "runs-on" {
            job.runs_on = Some(unquote_run_scalar(value)?);
        }
        if key == "container" {
            job.has_container = true;
        }
        return Ok(());
    }
    if indent == 6 {
        if job.section == JobSection::Steps && scan::content_is_step_item(key, value) {
            job.finish_step();
            job.current_step = Some(StepScan::default());
            return Ok(());
        }
        job.section = if job.section == JobSection::Defaults && key == "run" && value.is_empty() {
            JobSection::DefaultsRun
        } else {
            JobSection::Root
        };
        return Ok(());
    }
    if indent == 8 {
        return scan_job_nested_value(job, key, value);
    }
    if indent > 8
        && key == "run"
        && let Some(step) = job.current_step.as_ref()
        && job.section == JobSection::Steps
        && !matches!(step.nested_mapping.as_deref(), Some("env" | "with"))
    {
        return Err(shellcheck_fail("run_step_ancestry_unsupported"));
    }
    Ok(())
}

fn scan_job_nested_value(
    job: &mut JobScan,
    key: &str,
    value: &str,
) -> Result<(), OrchestratorError> {
    if job.section == JobSection::DefaultsRun && key == "shell" {
        job.default_shell = Some(shell::parse_shell(value)?);
        return Ok(());
    }
    if job.section != JobSection::Steps {
        return Ok(());
    }
    let Some(step) = job.current_step.as_mut() else {
        return if key == "run" {
            Err(shellcheck_fail("run_step_without_name"))
        } else {
            Ok(())
        };
    };
    match key {
        "run" => {
            if step.body.is_some() {
                return Err(shellcheck_fail("duplicate_step_run"));
            }
            let scalar = value.trim();
            if scalar.is_empty() || scalar.starts_with(['|', '>']) {
                return Err(shellcheck_fail("run_block_scalar_unlintable"));
            }
            step.body = Some(unquote_run_scalar(scalar)?);
            step.nested_mapping = None;
        }
        "shell" => {
            step.shell = Some(shell::parse_shell(value)?);
            step.nested_mapping = None;
        }
        "env" | "with" if value.is_empty() => {
            step.nested_mapping = Some(key.to_owned());
        }
        _ => step.nested_mapping = None,
    }
    Ok(())
}

impl JobScan {
    fn finish_step(&mut self) {
        if let Some(step) = self.current_step.take() {
            self.runs.push(step);
        }
    }

    fn finish(mut self) -> Self {
        self.finish_step();
        self
    }
}

fn collect_runs(
    jobs: Vec<JobScan>,
    workflow_shell: Option<ShellDialect>,
) -> Result<Vec<StagedRun>, OrchestratorError> {
    let mut runs = Vec::new();
    for job in jobs {
        for step in job.runs {
            let Some(body) = step.body else {
                continue;
            };
            shell::reject_shellcheck_shell_directive(&body)?;
            let shell = step
                .shell
                .or(job.default_shell)
                .or(workflow_shell)
                .or_else(|| job.has_container.then_some(ShellDialect::Sh))
                .or_else(|| hosted_runner_default(job.runs_on.as_deref()));
            let Some(shell) = shell else {
                return Err(shellcheck_fail("shell_unresolved_for_runner"));
            };
            runs.push(StagedRun { body, shell });
        }
    }
    Ok(runs)
}

fn hosted_runner_default(runs_on: Option<&str>) -> Option<ShellDialect> {
    let label = runs_on?;
    let is_known_linux = velnor_actions_contract::config::is_hosted_catalog(label);
    let is_known_macos = matches!(
        velnor_actions_contract::ReleaseTarget::for_runner_label(label),
        Some(
            velnor_actions_contract::ReleaseTarget::MacosArm64
                | velnor_actions_contract::ReleaseTarget::MacosX86_64
        )
    );
    if (is_known_linux && label.starts_with("ubuntu-")) || is_known_macos {
        // GitHub-hosted Ubuntu and macOS runners default `run` steps to Bash.
        Some(ShellDialect::Bash)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "validate_shell_yaml_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "validate_shell_yaml_runner_tests.rs"]
mod runner_tests;

#[cfg(test)]
#[path = "validate_shell_yaml_step_anchor_tests.rs"]
mod step_anchor_tests;
