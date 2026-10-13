use std::path::Path;

use crate::OrchestratorError;

use super::{shellcheck_fail, unquote_run_scalar};

mod shell;

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
struct WorkflowScan {
    section: WorkflowSection,
    saw_jobs: bool,
    workflow_shell: Option<ShellDialect>,
    current_job: Option<JobScan>,
    jobs: Vec<JobScan>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum WorkflowSection {
    #[default]
    Root,
    Defaults,
    DefaultsRun,
    Jobs,
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

#[derive(Default)]
struct StepScan {
    body: Option<String>,
    shell: Option<ShellDialect>,
    nested_mapping: Option<String>,
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
    let mut scan = WorkflowScan::default();
    for raw in text.lines() {
        let (indent, content) = indent_content(raw)?;
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let entry = mapping_entry(content);
        if indent == 6
            && content.starts_with("- ")
            && scan.section == WorkflowSection::Jobs
            && scan
                .current_job
                .as_ref()
                .is_some_and(|job| job.section == JobSection::Steps)
        {
            let Some((key, value)) = entry else {
                return Err(shellcheck_fail("step_mapping_unsupported"));
            };
            if !content_is_step_item(key, value) {
                return Err(shellcheck_fail("step_name_must_be_first"));
            }
        }
        let Some((key, value)) = entry else {
            continue;
        };
        if indent == 0 {
            if let Some(job) = scan.current_job.take() {
                scan.jobs.push(job.finish());
            }
            scan.section = match (key, value) {
                ("jobs", "") => {
                    scan.saw_jobs = true;
                    WorkflowSection::Jobs
                }
                ("defaults", "") => WorkflowSection::Defaults,
                _ => WorkflowSection::Root,
            };
            continue;
        }
        if scan.section != WorkflowSection::Jobs {
            scan_workflow_default(&mut scan, indent, key, value)?;
            continue;
        }
        if indent == 2 && value.is_empty() && !key.starts_with('-') {
            if let Some(job) = scan.current_job.take() {
                scan.jobs.push(job.finish());
            }
            scan.current_job = Some(JobScan::default());
            continue;
        }
        if key == "run"
            && scan.section == WorkflowSection::Jobs
            && let Some(job) = scan.current_job.as_ref()
            && job.section == JobSection::Steps
            && indent != 8
        {
            let is_action_input = indent > 8
                && job.current_step.as_ref().is_some_and(|step| {
                    matches!(step.nested_mapping.as_deref(), Some("env" | "with"))
                });
            if !is_action_input {
                return Err(shellcheck_fail("run_step_ancestry_unsupported"));
            }
        }
        let Some(job) = scan.current_job.as_mut() else {
            continue;
        };
        scan_job_line(job, indent, key, value)?;
    }
    if let Some(job) = scan.current_job.take() {
        scan.jobs.push(job.finish());
    }
    if !scan.saw_jobs {
        return Err(shellcheck_fail("workflow_jobs_missing"));
    }
    collect_runs(scan.jobs, scan.workflow_shell)
}

fn scan_workflow_default(
    scan: &mut WorkflowScan,
    indent: usize,
    key: &str,
    value: &str,
) -> Result<(), OrchestratorError> {
    match scan.section {
        WorkflowSection::Defaults if indent == 2 => {
            scan.section = if key == "run" && value.is_empty() {
                WorkflowSection::DefaultsRun
            } else {
                WorkflowSection::Root
            };
        }
        WorkflowSection::DefaultsRun if indent == 4 && key == "shell" => {
            scan.workflow_shell = Some(shell::parse_shell(value)?);
        }
        WorkflowSection::DefaultsRun if indent == 2 => {
            scan.section = WorkflowSection::Root;
        }
        _ => {}
    }
    Ok(())
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
        if job.section == JobSection::Steps && content_is_step_item(key, value) {
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
    let is_known_macos = label == "macos-15";
    if (is_known_linux && label.starts_with("ubuntu-")) || is_known_macos {
        // GitHub-hosted Ubuntu and macOS runners default `run` steps to Bash.
        Some(ShellDialect::Bash)
    } else {
        None
    }
}

fn indent_content(line: &str) -> Result<(usize, &str), OrchestratorError> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if line.as_bytes().get(indent) == Some(&b'\t') {
        return Err(shellcheck_fail("workflow_yaml_tabs_unsupported"));
    }
    Ok((indent, line[indent..].trim_end()))
}

fn mapping_entry(content: &str) -> Option<(&str, &str)> {
    let content = content.strip_prefix("- ").unwrap_or(content);
    let (key, value) = content.split_once(':')?;
    if key.is_empty() || key.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    Some((key, value.trim()))
}

fn content_is_step_item(key: &str, value: &str) -> bool {
    key == "name" && !value.is_empty()
}

#[cfg(test)]
mod tests;
