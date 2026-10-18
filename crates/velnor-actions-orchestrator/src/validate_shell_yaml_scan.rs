use std::collections::BTreeMap;

use crate::OrchestratorError;

use super::{
    JobScan, JobSection, ShellDialect, StagedRun, StepScan, alias_scope, run_anchor, shell,
    shellcheck_fail,
};

#[derive(Default)]
struct WorkflowScan {
    section: WorkflowSection,
    saw_jobs: bool,
    workflow_shell: Option<ShellDialect>,
    run_anchors: BTreeMap<String, String>,
    step_anchors: BTreeMap<String, StepScan>,
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

pub(super) fn scan_workflow(text: &str) -> Result<Vec<StagedRun>, OrchestratorError> {
    let mut scan = WorkflowScan::default();
    for raw in text.lines() {
        let (indent, content) = indent_content(raw)?;
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let step_sequence_item = indent == 6
            && content.starts_with("- ")
            && scan.section == WorkflowSection::Jobs
            && scan
                .current_job
                .as_ref()
                .is_some_and(|job| job.section == JobSection::Steps);
        if step_sequence_item && scan_step_mapping_item(&mut scan, content)? {
            continue;
        }
        let entry = mapping_entry(content);
        if step_sequence_item {
            let Some((key, value)) = entry else {
                return Err(shellcheck_fail("step_mapping_unsupported"));
            };
            if !content_is_step_item(key, value) {
                return Err(shellcheck_fail("step_name_must_be_first"));
            }
        }
        let Some((key, source_value)) = entry else {
            alias_scope::reject_non_mapping_content(content)?;
            continue;
        };
        let is_step_run = is_step_run_site(&scan, indent, key);
        alias_scope::validate_mapping_value(key, source_value, is_step_run)?;
        let value = if is_step_run {
            run_anchor::resolve_run_scalar(source_value, &mut scan.run_anchors)?
        } else {
            source_value.to_owned()
        };
        if advance_workflow_scope(&mut scan, indent, key, &value)? {
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
        if indent == 8
            && job.section == JobSection::Steps
            && let Some(step) = job.current_step.as_mut()
            && step.anchor.is_some()
            && !step.first_mapping_field_seen
        {
            if key != "name" || value.is_empty() {
                return Err(shellcheck_fail("step_mapping_name_must_be_first"));
            }
            step.first_mapping_field_seen = true;
        }
        super::scan_job_line(job, indent, key, &value)?;
    }
    finish_workflow_scan(scan)
}

fn advance_workflow_scope(
    scan: &mut WorkflowScan,
    indent: usize,
    key: &str,
    value: &str,
) -> Result<bool, OrchestratorError> {
    if indent == 0 {
        scan.finish_current_step()?;
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
        return Ok(true);
    }
    if scan.section != WorkflowSection::Jobs {
        scan_workflow_default(scan, indent, key, value)?;
        return Ok(true);
    }
    if indent == 2 && value.is_empty() && !key.starts_with('-') {
        scan.finish_current_step()?;
        if let Some(job) = scan.current_job.take() {
            scan.jobs.push(job.finish());
        }
        scan.current_job = Some(JobScan::default());
        return Ok(true);
    }
    Ok(false)
}

fn finish_workflow_scan(mut scan: WorkflowScan) -> Result<Vec<StagedRun>, OrchestratorError> {
    scan.finish_current_step()?;
    if let Some(job) = scan.current_job.take() {
        scan.jobs.push(job.finish());
    }
    if !scan.saw_jobs {
        return Err(shellcheck_fail("workflow_jobs_missing"));
    }
    super::collect_runs(scan.jobs, scan.workflow_shell)
}

fn scan_step_mapping_item(
    scan: &mut WorkflowScan,
    content: &str,
) -> Result<bool, OrchestratorError> {
    // Finish the previous mapping so its anchor is available before the next item.
    scan.finish_current_step()?;
    if let Some(name) = step_mapping_anchor(content)? {
        let Some(job) = scan.current_job.as_mut() else {
            return Err(shellcheck_fail("step_mapping_anchor_without_job"));
        };
        job.current_step = Some(StepScan {
            anchor: Some(name.to_owned()),
            ..StepScan::default()
        });
        return Ok(true);
    }
    if let Some(name) = step_mapping_alias(content)? {
        let Some(step) = scan.step_anchors.get(name).cloned() else {
            return Err(OrchestratorError::Validation {
                tool: "shellcheck".to_owned(),
                problem: format!("step_mapping_alias_unresolved:{name}"),
            });
        };
        let Some(job) = scan.current_job.as_mut() else {
            return Err(shellcheck_fail("step_mapping_alias_without_job"));
        };
        job.runs.push(step);
        return Ok(true);
    }
    Ok(false)
}

impl WorkflowScan {
    fn finish_current_step(&mut self) -> Result<(), OrchestratorError> {
        let Some(step) = self
            .current_job
            .as_mut()
            .and_then(|job| job.current_step.take())
        else {
            return Ok(());
        };
        if let Some(name) = &step.anchor {
            if !step.first_mapping_field_seen {
                return Err(shellcheck_fail("step_mapping_name_missing"));
            }
            if self
                .step_anchors
                .insert(name.clone(), step.clone())
                .is_some()
            {
                return Err(shellcheck_fail("step_mapping_anchor_duplicate"));
            }
        }
        let Some(job) = self.current_job.as_mut() else {
            return Err(shellcheck_fail("step_mapping_without_job"));
        };
        job.runs.push(step);
        Ok(())
    }
}

fn is_step_run_site(scan: &WorkflowScan, indent: usize, key: &str) -> bool {
    indent == 8
        && key == "run"
        && scan.section == WorkflowSection::Jobs
        && scan
            .current_job
            .as_ref()
            .is_some_and(|job| job.section == JobSection::Steps && job.current_step.is_some())
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

fn step_mapping_anchor(content: &str) -> Result<Option<&str>, OrchestratorError> {
    let Some(name) = content.strip_prefix("- &") else {
        return Ok(None);
    };
    if !valid_anchor_name(name) {
        return Err(shellcheck_fail("step_mapping_anchor_malformed"));
    }
    Ok(Some(name))
}

fn step_mapping_alias(content: &str) -> Result<Option<&str>, OrchestratorError> {
    let Some(name) = content.strip_prefix("- *") else {
        return Ok(None);
    };
    if !valid_anchor_name(name) {
        return Err(shellcheck_fail("step_mapping_alias_malformed"));
    }
    Ok(Some(name))
}

fn valid_anchor_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) fn content_is_step_item(key: &str, value: &str) -> bool {
    key == "name" && !value.is_empty()
}
