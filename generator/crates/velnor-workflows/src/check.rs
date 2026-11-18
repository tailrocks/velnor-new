// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};
use crate::expression::{Scope, check, check_condition, is_identifier, whole_expression};
use crate::plan::{
    Action, Body, DEFAULT_RUNNER, DEFAULT_TIMEOUT_MINUTES, Env, Input, Job, Matrix, Step, Workflow,
};
use crate::raw::{RawJob, RawStep, RawWorkflow};

const RESERVED_FILES: [&str; 2] = ["rust-ci", "release"];
const PERMISSION_SCOPES: [&str; 13] = [
    "actions",
    "artifact-metadata",
    "attestations",
    "checks",
    "contents",
    "deployments",
    "id-token",
    "issues",
    "packages",
    "pages",
    "pull-requests",
    "security-events",
    "statuses",
];
const MAX_TIMEOUT_MINUTES: u16 = 360;

pub(crate) fn workflow(raw: RawWorkflow) -> Result<Workflow, WorkflowsError> {
    let file = raw.file;
    let valid_file = !file.is_empty()
        && file
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !file.starts_with('-');
    if !valid_file {
        return refuse(format!(
            "the workflow file {file:?} must be lower case letters, digits and dashes"
        ));
    }
    if RESERVED_FILES.contains(&file.as_str()) {
        return refuse(format!(
            "the workflow file {file} belongs to another generator output"
        ));
    }
    if raw.name.trim().is_empty() {
        return refuse(format!("the workflow {file} needs a name"));
    }
    let mut inputs: Vec<Input> = Vec::new();
    for input in raw.input {
        if !is_identifier(&input.name, true) || inputs.iter().any(|seen| seen.name == input.name) {
            return refuse(format!(
                "the input {:?} of {file} is not a unique identifier",
                input.name
            ));
        }
        inputs.push(Input {
            name: input.name,
            description: input.description,
            required: input.required.unwrap_or(true),
        });
    }
    let mut jobs: Vec<Job> = Vec::new();
    for job in raw.job {
        let checked = self::job(job, raw.condition.as_deref(), &inputs, &jobs)?;
        jobs.push(checked);
    }
    if jobs.is_empty() {
        return refuse(format!(
            "the workflow {file} needs at least one [[workflow.job]]"
        ));
    }
    Ok(Workflow {
        file,
        name: raw.name,
        runner: raw.runner.unwrap_or_else(|| DEFAULT_RUNNER.to_owned()),
        condition: raw
            .condition
            .map(|text| crate::expression::bare(&text).to_owned()),
        inputs,
        jobs,
    })
}

fn job(
    raw: RawJob,
    workflow_condition: Option<&str>,
    inputs: &[Input],
    earlier: &[Job],
) -> Result<Job, WorkflowsError> {
    let id = raw.id;
    if !is_identifier(&id, true) || earlier.iter().any(|seen| seen.id == id) {
        return refuse(format!("the job id {id:?} is not a unique identifier"));
    }
    for need in &raw.needs {
        if !earlier.iter().any(|seen| &seen.id == need) {
            return refuse(format!(
                "job {id} needs {need}, which is not declared before it"
            ));
        }
    }
    let timeout_minutes = raw.timeout_minutes.unwrap_or(DEFAULT_TIMEOUT_MINUTES);
    if timeout_minutes == 0 || timeout_minutes > MAX_TIMEOUT_MINUTES {
        return refuse(format!(
            "job {id} timeout_minutes must be 1 to {MAX_TIMEOUT_MINUTES}"
        ));
    }
    declared_access(&id, &raw.permissions, raw.environment.as_deref())?;
    let matrix = match raw.matrix {
        None => None,
        Some(matrix) => Some(self::matrix(&id, &matrix, &raw.needs, earlier)?),
    };
    let needed_outputs: Vec<(&str, &str)> = earlier
        .iter()
        .filter(|seen| raw.needs.contains(&seen.id))
        .flat_map(|seen| {
            seen.outputs
                .iter()
                .map(|(name, _)| (seen.id.as_str(), name.as_str()))
        })
        .collect();
    let input_names: Vec<&str> = inputs.iter().map(|input| input.name.as_str()).collect();
    let mut scope = Scope {
        inputs: &input_names,
        has_matrix: matrix.is_some(),
        needs: &raw.needs,
        needed_outputs: &needed_outputs,
        steps: Vec::new(),
    };
    if let Some(condition) = workflow_condition {
        check_condition(condition, &scope)?;
    }
    let mut steps: Vec<Step> = Vec::new();
    for step in raw.step {
        let checked = self::step(&id, raw.helper, step, &scope, &steps)?;
        scope.steps.extend(checked.id.clone());
        steps.push(checked);
    }
    if steps.is_empty() {
        return refuse(format!("job {id} needs at least one [[workflow.job.step]]"));
    }
    let mut outputs = Vec::new();
    for (name, value) in raw.outputs {
        outputs.push((name.clone(), output(&id, &name, &value, &steps)?));
    }
    Ok(Job {
        id,
        needs: raw.needs,
        helper: raw.helper,
        environment: raw.environment,
        timeout_minutes,
        permissions: raw.permissions.into_iter().collect(),
        outputs,
        matrix,
        steps,
    })
}

fn declared_access(
    id: &str,
    permissions: &std::collections::BTreeMap<String, String>,
    environment: Option<&str>,
) -> Result<(), WorkflowsError> {
    for (scope, access) in permissions {
        let level = matches!(access.as_str(), "read" | "write" | "none")
            && !(scope == "id-token" && access == "read");
        if !PERMISSION_SCOPES.contains(&scope.as_str()) || !level {
            return refuse(format!(
                "job {id} permission {scope} = {access:?} is not a known scope with read, write or none (id-token has no read)"
            ));
        }
    }
    if environment.is_some_and(|name| name.trim().is_empty()) {
        return refuse(format!("job {id} environment must not be empty"));
    }
    Ok(())
}

fn matrix(
    id: &str,
    raw: &crate::raw::RawMatrix,
    needs: &[String],
    earlier: &[Job],
) -> Result<Matrix, WorkflowsError> {
    let Some((from_job, from_output)) = raw.from_output.split_once('.') else {
        return refuse(format!(
            "job {id} matrix from_output must be <job>.<output>"
        ));
    };
    if !needs.iter().any(|need| need == from_job) {
        return refuse(format!(
            "job {id} builds its matrix from the output of {from_job}, so it must list {from_job} in needs"
        ));
    }
    let declared = earlier
        .iter()
        .find(|seen| seen.id == from_job)
        .is_some_and(|seen| seen.outputs.iter().any(|(name, _)| name == from_output));
    if !declared {
        return refuse(format!(
            "job {id} builds its matrix from {from_job}.{from_output}, which {from_job} does not declare as an output"
        ));
    }
    if raw.max_parallel == Some(0) {
        return refuse(format!("job {id} matrix max_parallel must be at least 1"));
    }
    Ok(Matrix {
        from_job: from_job.to_owned(),
        from_output: from_output.to_owned(),
        fail_fast: raw.fail_fast,
        max_parallel: raw.max_parallel,
    })
}

fn output(job: &str, name: &str, value: &str, steps: &[Step]) -> Result<String, WorkflowsError> {
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

fn step(
    job: &str,
    helper_installed: bool,
    raw: RawStep,
    scope: &Scope<'_>,
    earlier: &[Step],
) -> Result<Step, WorkflowsError> {
    if let Some(id) = &raw.id
        && (!is_identifier(id, true) || earlier.iter().any(|seen| seen.id.as_ref() == Some(id)))
    {
        return refuse(format!(
            "job {job} step id {id:?} is not a unique identifier"
        ));
    }
    let (label, body) = match (raw.run, raw.uses, raw.helper) {
        (Some(run), None, None) => {
            if raw
                .name
                .as_deref()
                .is_none_or(|name| name.trim().is_empty())
                || run.trim().is_empty()
            {
                return refuse(format!("a run step of job {job} needs a name and a run"));
            }
            (String::new(), Body::Run(run))
        }
        (None, Some(name), None) => {
            let Some(action) = Action::parse(&name) else {
                return refuse(format!(
                    "job {job} uses {name:?}, which is not one of the pinned actions {:?}",
                    Action::NAMES
                ));
            };
            (name, Body::Uses(action))
        }
        (None, None, Some(command)) => {
            if !is_identifier(&command, true) {
                return refuse(format!("job {job} helper {command:?} is not an identifier"));
            }
            if !helper_installed {
                return refuse(format!(
                    "job {job} runs the helper command {command}, so it needs helper = true"
                ));
            }
            (command.clone(), Body::Helper(command))
        }
        _ => {
            return refuse(format!(
                "a step of job {job} needs exactly one of run, uses and helper"
            ));
        }
    };
    if !raw.with.is_empty() && !matches!(body, Body::Uses(_)) {
        return refuse(format!("a step of job {job} sets with without uses"));
    }
    let condition = raw
        .condition
        .map(|text| check_condition(&text, scope))
        .transpose()?;
    Ok(Step {
        id: raw.id,
        name: raw.name.unwrap_or(label),
        condition,
        body,
        with: values(job, raw.with, scope)?,
        continue_on_error: raw.continue_on_error,
        env: values(job, raw.env, scope)?,
    })
}

fn values(
    job: &str,
    table: std::collections::BTreeMap<String, String>,
    scope: &Scope<'_>,
) -> Result<Vec<(String, Env)>, WorkflowsError> {
    let mut items = Vec::new();
    for (key, value) in table {
        if !is_identifier(&key, true) {
            return refuse(format!("job {job} key {key:?} is not an identifier"));
        }
        if value.contains("${{") {
            check(&value, scope)?;
            items.push((key, Env::Expression(whole_inner(&value))));
        } else {
            items.push((key, Env::Text(value)));
        }
    }
    Ok(items)
}

fn whole_inner(value: &str) -> String {
    whole_expression(value).unwrap_or_default().to_owned()
}
