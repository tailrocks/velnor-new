// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{token_permission_accesses, token_permission_accesses_prose};

use crate::call::{rank, refuse_step_keys};
use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::job_limits;
use crate::plan::{Body, DEFAULT_TIMEOUT_MINUTES, Job, Output, Step};
use crate::raw::RawJob;

const MAX_TIMEOUT_MINUTES: u16 = 360;

pub(crate) fn check_outputs(
    file: &str,
    outputs: &[Output],
    jobs: &[Job],
) -> Result<(), WorkflowsError> {
    for (index, output) in outputs.iter().enumerate() {
        let declared = jobs
            .iter()
            .find(|job| job.id == output.from_job)
            .is_some_and(|job| job.output_names().any(|name| name == output.from_output));
        if !declared || outputs[..index].iter().any(|seen| seen.name == output.name) {
            return refuse(format!(
                "the workflow output {} of {file} must be a unique name with value <job>.<output> of a declared job output",
                output.name
            ));
        }
    }
    Ok(())
}

pub(crate) fn job_shape(raw: &RawJob, earlier: &[Job]) -> Result<u16, WorkflowsError> {
    let id = &raw.id;
    if !is_identifier(id, true) || earlier.iter().any(|seen| seen.id == *id) {
        return refuse(format!("the job id {id:?} is not a unique identifier"));
    }
    for need in &raw.needs {
        if !earlier.iter().any(|seen| &seen.id == need) {
            return refuse(format!(
                "job {id} needs {need}, which is not declared before it"
            ));
        }
    }
    if raw.uses.is_some() {
        refuse_step_keys(raw)?;
    } else if raw.step.is_empty() {
        return refuse(format!("job {id} needs at least one [[workflow.job.step]]"));
    }
    let timeout_minutes = raw.timeout_minutes.unwrap_or(DEFAULT_TIMEOUT_MINUTES);
    if timeout_minutes == 0 || timeout_minutes > MAX_TIMEOUT_MINUTES {
        return refuse(format!(
            "job {id} timeout_minutes must be 1 to {MAX_TIMEOUT_MINUTES}"
        ));
    }
    Ok(timeout_minutes)
}

pub(crate) fn needed_outputs<'a>(needs: &[String], earlier: &'a [Job]) -> Vec<(&'a str, &'a str)> {
    earlier
        .iter()
        .filter(|seen| needs.contains(&seen.id))
        .flat_map(|seen| seen.output_names().map(|name| (seen.id.as_str(), name)))
        .collect()
}

pub(crate) fn require_permissions(
    job: &str,
    steps: &[Step],
    permissions: &[(String, String)],
) -> Result<(), WorkflowsError> {
    for step in steps {
        let Body::Uses(action) = &step.body else {
            continue;
        };
        for (scope, access) in action.needs_permissions() {
            let granted = permissions
                .iter()
                .any(|(name, level)| name == scope && rank(level) >= rank(access));
            if !granted {
                return refuse(format!(
                    "job {job} uses {}, which needs the permission {scope} = {access:?}; declare it on the job",
                    step.name
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn declared_access(
    id: &str,
    permissions: &BTreeMap<String, String>,
    environment: Option<&str>,
) -> Result<(), WorkflowsError> {
    for (scope, access) in permissions {
        let allowed = token_permission_accesses(scope);
        if !allowed.is_some_and(|accesses| accesses.contains(&access.as_str())) {
            return refuse(match allowed {
                Some(_) => format!(
                    "job {id} permission {scope} = {access:?} is not valid; {scope} takes {}",
                    token_permission_accesses_prose(scope)
                ),
                None => format!("job {id} permission {scope} is not a GitHub token scope"),
            });
        }
    }
    job_limits::environment(id, environment)
}

pub(crate) fn output(
    job: &str,
    name: &str,
    value: &str,
    steps: &[Step],
) -> Result<String, WorkflowsError> {
    let reference = value.split('.').collect::<Vec<_>>();
    let known = match reference.as_slice() {
        ["steps", step, "outputs", output] => {
            is_identifier(output, true) && steps.iter().any(|seen| seen.id.as_deref() == Some(step))
        }
        _ => false,
    };
    if !is_identifier(name, true) || !known {
        return refuse(format!(
            "job {job} output {name} must be steps.<id of a step of the job>.outputs.<name>, got {value:?}"
        ));
    }
    Ok(value.to_owned())
}
