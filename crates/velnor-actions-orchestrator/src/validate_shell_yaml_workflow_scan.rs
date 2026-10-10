use crate::OrchestratorError;

use super::run_anchor;
use super::{
    JobScan, StagedRun, StepScan, WorkflowScan, alias_scope, finish_current_step,
    finish_workflow_scan, indent_content, is_step_run_site, is_step_sequence_item,
    is_supported_mapping_site, mapping_entry, reserve_anchor_name, scan_job_line,
    scan_workflow_default, shellcheck_fail,
};

pub(super) fn scan_workflow(text: &str) -> Result<Vec<StagedRun>, OrchestratorError> {
    let mut scan = WorkflowScan::default();
    for raw in text.lines() {
        let (indent, content) = indent_content(raw)?;
        if content.is_empty() || content.starts_with('#') {
            continue;
        }

        if let Some((name, parent_indent)) = scan.pending_mapping_anchor.take() {
            if indent != parent_indent + 2
                || content.starts_with("- ")
                || mapping_entry(content).is_none()
            {
                return Err(shellcheck_fail("workflow_mapping_anchor_not_a_mapping"));
            }
            scan.mapping_anchors.insert(name);
        }

        let step_sequence_item = is_step_sequence_item(&scan, indent, content);
        if step_sequence_item {
            finish_current_step(&mut scan)?;
            if let Some(property) = alias_scope::parse_step_property(content)? {
                match property {
                    alias_scope::StepProperty::Anchor(name) => {
                        reserve_anchor_name(&mut scan, &name)?;
                        let Some(job) = scan.current_job.as_mut() else {
                            return Err(shellcheck_fail("workflow_step_anchor_outside_steps"));
                        };
                        job.current_step = Some(StepScan {
                            anchor_definition: Some(name),
                            ..StepScan::default()
                        });
                    }
                    alias_scope::StepProperty::Alias(name) => {
                        let Some(step) = scan.step_anchors.get(&name).cloned() else {
                            return Err(shellcheck_fail("workflow_step_alias_unresolved"));
                        };
                        let Some(job) = scan.current_job.as_mut() else {
                            return Err(shellcheck_fail("workflow_step_alias_outside_steps"));
                        };
                        job.runs.push(step);
                    }
                }
                continue;
            }
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
        let Some((key, source_value)) = entry else {
            alias_scope::reject_non_mapping_content(content)?;
            continue;
        };
        let is_step_run = is_step_run_site(&scan, indent, key);
        let is_supported_mapping = is_supported_mapping_site(&scan, indent, key);
        alias_scope::validate_mapping_value(key, source_value, is_step_run, is_supported_mapping)?;
        let mut value = source_value.to_owned();
        if is_supported_mapping {
            match alias_scope::parse_mapping_property(source_value)? {
                Some(alias_scope::MappingProperty::Anchor { name, inline_empty }) => {
                    reserve_anchor_name(&mut scan, &name)?;
                    if inline_empty {
                        scan.mapping_anchors.insert(name);
                    } else {
                        scan.pending_mapping_anchor = Some((name, indent));
                    }
                    value.clear();
                }
                Some(alias_scope::MappingProperty::Alias(name)) => {
                    if !scan.mapping_anchors.contains(&name) {
                        return Err(shellcheck_fail("workflow_mapping_alias_unresolved"));
                    }
                    value.clear();
                }
                None => {}
            }
        }
        let value = if is_step_run {
            run_anchor::resolve_run_scalar(
                source_value,
                &mut scan.run_anchors,
                &mut scan.used_anchor_names,
            )?
        } else {
            value
        };
        if indent == 0 {
            finish_current_step(&mut scan)?;
            if let Some(job) = scan.current_job.take() {
                scan.jobs.push(job.finish());
            }
            scan.section = match (key, value.as_str()) {
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
            scan_workflow_default(&mut scan, indent, key, &value)?;
            continue;
        }
        if indent == 2 && value.is_empty() && !key.starts_with('-') {
            finish_current_step(&mut scan)?;
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
        scan_job_line(job, indent, key, &value)?;
    }
    finish_workflow_scan(scan)
}
