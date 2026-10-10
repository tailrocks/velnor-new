//! Shared local composites for typed declared-task report envelopes.
//!
//! `TaskExecution` carries argv, task/report identity, and env as distinct
//! validated fields. This renderer never recovers task metadata from shell
//! source or accepts an environment marker as proof of eligibility.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn, Step, StepKind, StepRole};

use crate::{
    RenderError, action_ref::DECLARED_TASK_ACTION_PREFIX, composite, marker, steps, toolchain_env,
    tree::RenderedFile, yaml::Yaml,
};

const ACTION_NAME_PREFIX: &str = "declared-task-";
const TASK_ID_INPUT: &str = "task_id";
const TASK_DIGEST_INPUT: &str = "task_digest";
const MATRIX_ID_INPUT: &str = "matrix_id";
const MATRIX_KEY_INPUT: &str = "matrix_key";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    argv_count: usize,
    env_keys: Vec<String>,
    helper_version: String,
}

/// Replace typed task steps and emit one composite per structural shape.
///
/// Every typed step must be on Ubuntu and follow the configured checkout. A
/// task which violates either precondition fails closed instead of silently
/// taking a shell fallback. The caller name and condition remain unchanged.
pub(crate) fn factor_obligation_steps(
    jobs: &BTreeMap<String, Job>,
    checkout_uses: &str,
    version: &str,
) -> Result<(BTreeMap<String, Job>, Vec<RenderedFile>), RenderError> {
    let mut eligible = BTreeMap::<(String, usize), TaskExecutionRef<'_>>::new();
    let mut shapes = BTreeSet::new();
    for (job_id, job) in jobs {
        for (step_index, step) in job.steps.iter().enumerate() {
            let StepKind::TaskExecution {
                argv,
                env,
                task_id,
                task_digest,
                matrix_id,
                matrix_key,
                report_helper_version,
                matrix_max_parallel: _,
                toolchain_inputs: _,
            } = &step.kind
            else {
                continue;
            };
            let hosted_ubuntu = matches!(RunsOn::parse(&job.runs_on), Ok(RunsOn::Hosted(label))
                if label.starts_with("ubuntu-")
                    && velnor_actions_contract::config::RUNNER_LABEL_CATALOG
                        .contains(&label.as_str()));
            if !hosted_ubuntu {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_requires_ubuntu:{job_id}"
                )));
            }
            if job.check_runner.is_some()
                || !job
                    .needs
                    .iter()
                    .any(|need| need == crate::render::PLAN_JOB_ID)
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_job_scope_mismatch:{job_id}"
                )));
            }
            if step.id.is_some() || step.role.is_some() || step.condition.is_none() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_authority_mismatch:{job_id}"
                )));
            }
            if report_helper_version != version {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_helper_version_mismatch:{job_id}"
                )));
            }
            let checkout_at = job.steps[..step_index].iter().position(|previous| {
                velnor_actions_contract::workflow::step_identity::is_configured_checkout(
                    previous,
                    checkout_uses,
                )
            });
            let Some(checkout_at) = checkout_at else {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_requires_checkout:{job_id}"
                )));
            };
            let staged_binary = format!("{}{report_helper_version}", steps::STAGED_BINARY_PREFIX);
            let helper_staged_after_checkout = job.steps[checkout_at + 1..step_index]
                .iter()
                .any(|previous| helper_staged_by(previous, &staged_binary));
            if !helper_staged_after_checkout {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_requires_staged_helper:{job_id}"
                )));
            }
            validate_task_fields(argv, env, task_id, task_digest, matrix_id, matrix_key)?;
            let shape = Shape {
                argv_count: argv.len(),
                env_keys: env.keys().cloned().collect(),
                helper_version: report_helper_version.clone(),
            };
            shapes.insert(shape);
            eligible.insert(
                (job_id.clone(), step_index),
                TaskExecutionRef {
                    argv,
                    env,
                    task_id,
                    task_digest,
                    matrix_id,
                    matrix_key,
                    helper_version: report_helper_version,
                },
            );
        }
    }
    let shape_ids: BTreeMap<Shape, usize> = shapes
        .into_iter()
        .enumerate()
        .map(|(index, shape)| (shape, index))
        .collect();
    let mut next = jobs.clone();
    for ((job_id, step_index), task) in &eligible {
        let shape = Shape {
            argv_count: task.argv.len(),
            env_keys: task.env.keys().cloned().collect(),
            helper_version: task.helper_version.clone(),
        };
        let action_id = shape_ids.get(&shape).ok_or_else(|| {
            RenderError::InvalidWorkflow("declared_task_shape_missing".to_owned())
        })?;
        let original = jobs
            .get(job_id)
            .and_then(|job| job.steps.get(*step_index))
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        let action = declared_task_call(*action_id, task, original)?;
        let job = next
            .get_mut(job_id)
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_job_missing".to_owned()))?;
        let caller = job
            .steps
            .get_mut(*step_index)
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        *caller = action;
    }
    let mut files = Vec::with_capacity(shape_ids.len());
    for (shape, action_id) in &shape_ids {
        files.push(declared_task_file(*action_id, shape, version)?);
    }
    Ok((next, files))
}

fn helper_staged_by(step: &Step, staged_binary: &str) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    (crate::closure::is_acquire_step(step) || step.role == Some(StepRole::PreseedStage))
        && run.iter().any(|argument| argument.contains(staged_binary))
}

struct TaskExecutionRef<'a> {
    argv: &'a [String],
    env: &'a BTreeMap<String, String>,
    task_id: &'a str,
    task_digest: &'a str,
    matrix_id: &'a str,
    matrix_key: &'a str,
    helper_version: &'a String,
}

fn validate_task_fields(
    argv: &[String],
    env: &BTreeMap<String, String>,
    task_id: &str,
    task_digest: &str,
    matrix_id: &str,
    matrix_key: &str,
) -> Result<(), RenderError> {
    crate::commands::validate_command_argv(argv)?;
    crate::commands::validate_env(env)?;
    crate::toolchain_env::reject_denied_step_keys(env)?;
    for (index, argument) in argv.iter().enumerate() {
        validate_input(&argv_input(index), argument)?;
    }
    for (key, value) in env {
        validate_input(&env_input(key), value)?;
    }
    for (key, value) in [
        (TASK_ID_INPUT, task_id),
        (TASK_DIGEST_INPUT, task_digest),
        (MATRIX_ID_INPUT, matrix_id),
        (MATRIX_KEY_INPUT, matrix_key),
    ] {
        validate_input(key, value)?;
    }
    Ok(())
}

fn declared_task_call(
    action_id: usize,
    task: &TaskExecutionRef<'_>,
    original: &Step,
) -> Result<Step, RenderError> {
    let mut with = BTreeMap::new();
    for (index, argument) in task.argv.iter().enumerate() {
        with.insert(argv_input(index), argument.clone());
    }
    for (key, value) in task.env {
        with.insert(env_input(key), value.clone());
    }
    for (key, value) in [
        (TASK_ID_INPUT, task.task_id),
        (TASK_DIGEST_INPUT, task.task_digest),
        (MATRIX_ID_INPUT, task.matrix_id),
        (MATRIX_KEY_INPUT, task.matrix_key),
    ] {
        with.insert(key.to_owned(), value.to_owned());
    }
    Ok(Step {
        name: original.name.clone(),
        id: original.id,
        role: original.role,
        condition: original.condition.clone(),
        kind: StepKind::Action {
            uses: format!("{DECLARED_TASK_ACTION_PREFIX}{action_id}"),
            with,
            env: BTreeMap::new(),
        },
    })
}

fn validate_input(key: &str, value: &str) -> Result<(), RenderError> {
    crate::expressions::check_with_key(key)?;
    crate::expressions::check_with_value(key, value)?;
    steps::scan_for_private_subcommands(key)?;
    steps::scan_for_private_subcommands(value)
}

fn declared_task_file(
    action_id: usize,
    shape: &Shape,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    let body = declared_task_document(action_id, shape)?;
    let bytes = marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{ACTION_NAME_PREFIX}{action_id}/action.yml"),
        bytes,
    })
}

fn declared_task_document(action_id: usize, shape: &Shape) -> Result<Yaml, RenderError> {
    let mut inputs = Vec::new();
    for index in 0..shape.argv_count {
        inputs.push(input_definition(argv_input(index)));
    }
    for key in &shape.env_keys {
        inputs.push(input_definition(env_input(key)));
    }
    for key in [
        TASK_ID_INPUT,
        TASK_DIGEST_INPUT,
        MATRIX_ID_INPUT,
        MATRIX_KEY_INPUT,
    ] {
        inputs.push(input_definition(key.to_owned()));
    }
    let mut env = toolchain_env::credential_scrub();
    for index in 0..shape.argv_count {
        env.insert(
            argv_env(index),
            format!("${{{{ inputs.{} }}}}", argv_input(index)),
        );
    }
    for key in &shape.env_keys {
        env.insert(key.clone(), format!("${{{{ inputs.{} }}}}", env_input(key)));
    }
    for (key, input) in [
        ("VELNOR_TASK_ID", TASK_ID_INPUT),
        ("VELNOR_TASK_DIGEST", TASK_DIGEST_INPUT),
        ("VELNOR_MATRIX_ID", MATRIX_ID_INPUT),
        ("VELNOR_MATRIX_KEY", MATRIX_KEY_INPUT),
    ] {
        env.insert(key.to_owned(), format!("${{{{ inputs.{input} }}}}"));
    }
    crate::commands::validate_composite_env(&env)?;
    let script = task_script(shape.argv_count, &shape.helper_version);
    let step = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Run declared task and write its result"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            crate::document_steps::string_map_yaml(&env),
        ),
        ("run".to_owned(), Yaml::str(script)),
    ]);
    composite::composite_yaml_with_inputs(
        &format!("{ACTION_NAME_PREFIX}{action_id}"),
        inputs,
        vec![step],
    )
}

fn input_definition(name: String) -> (String, Yaml) {
    (
        name,
        Yaml::Map(vec![
            (
                "description".to_owned(),
                Yaml::str("Fixed declared task argument or environment value."),
            ),
            ("required".to_owned(), Yaml::Bool(true)),
        ]),
    )
}

fn argv_input(index: usize) -> String {
    format!("argv_{index}")
}

fn env_input(key: &str) -> String {
    format!("env_{key}")
}

fn argv_env(index: usize) -> String {
    format!("VELNOR_WRAPPER_ARGV_{index}")
}

fn task_script(argv_count: usize, helper_version: &str) -> String {
    let argv = (0..argv_count)
        .map(|index| format!("\"$VELNOR_WRAPPER_ARGV_{index}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let helper = format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{helper_version}");
    format!(
        "unset {};\nset +e\nstarted_ms=$(date +%s%3N)\nargv=( {argv} )\n\"${{argv[@]}}\"\ntask_code=$?\nVELNOR_EXIT_CODE=\"$task_code\" VELNOR_START_MS=\"$started_ms\" VELNOR_INTERNAL_OP=write-task-report-v1 \"{helper}\"\nreport_code=$?\nif [ \"$task_code\" -ne 0 ]; then exit \"$task_code\"; fi\nexit \"$report_code\"",
        toolchain_env::CREDENTIAL_UNSET_VARS.join(" ")
    )
}

#[cfg(test)]
#[path = "task_wrapper_tests.rs"]
mod tests;
