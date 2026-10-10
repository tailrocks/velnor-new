//! Shared local composites for exact declared-task report wrappers.
//!
//! Only the fixed report envelope around a static `mise exec` argv is
//! factored. Dynamic shell syntax, credential-bearing env, non-Ubuntu jobs,
//! and typed-role steps remain in their original workflow position.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    RenderError, action_ref::DECLARED_TASK_ACTION_PREFIX, composite, marker, steps, toolchain_env,
    tree::RenderedFile, yaml::Yaml,
};
#[path = "task_wrapper_parse.rs"]
mod parse;
use parse::{ParsedTask, parse_step};

const ACTION_NAME_PREFIX: &str = "declared-task-";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    argv_count: usize,
    env_keys: Vec<String>,
    helper: String,
}

/// Replace eligible obligation steps and emit one composite per typed shape.
///
/// The caller name and condition remain on the workflow step. Each argv word
/// and non-credential environment value crosses the action boundary as data;
/// no value is interpolated into executable shell text.
pub(crate) fn factor_obligation_steps(
    jobs: &BTreeMap<String, Job>,
    checkout_uses: &str,
    version: &str,
) -> Result<(BTreeMap<String, Job>, Vec<RenderedFile>), RenderError> {
    let mut eligible = BTreeMap::<(String, usize), ParsedTask>::new();
    let mut shapes = BTreeSet::new();
    for (job_id, job) in jobs {
        if !job.runs_on.starts_with("ubuntu-") {
            continue;
        }
        for (step_index, step) in job.steps.iter().enumerate() {
            if !job.steps[..step_index].iter().any(|previous| {
                matches!(&previous.kind, StepKind::Action { uses, .. } if uses == checkout_uses)
            }) {
                continue;
            }
            let Some(parsed) = parse_step(step)? else {
                continue;
            };
            let shape = Shape {
                argv_count: parsed.argv.len(),
                env_keys: parsed.env.keys().cloned().collect(),
                helper: parsed.helper.clone(),
            };
            shapes.insert(shape);
            eligible.insert((job_id.clone(), step_index), parsed);
        }
    }
    let shape_ids: BTreeMap<Shape, usize> = shapes
        .into_iter()
        .enumerate()
        .map(|(index, shape)| (shape, index))
        .collect();
    let mut next = jobs.clone();
    for ((job_id, step_index), parsed) in &eligible {
        let shape = Shape {
            argv_count: parsed.argv.len(),
            env_keys: parsed.env.keys().cloned().collect(),
            helper: parsed.helper.clone(),
        };
        let action_id = shape_ids.get(&shape).ok_or_else(|| {
            RenderError::InvalidWorkflow("declared_task_shape_missing".to_owned())
        })?;
        let original = jobs
            .get(job_id)
            .and_then(|job| job.steps.get(*step_index))
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        let action = declared_task_call(*action_id, parsed, original)?;
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

fn declared_task_call(
    action_id: usize,
    parsed: &ParsedTask,
    original: &Step,
) -> Result<Step, RenderError> {
    let uses = format!("{DECLARED_TASK_ACTION_PREFIX}{action_id}");
    let mut with = BTreeMap::new();
    for (index, argument) in parsed.argv.iter().enumerate() {
        let key = argv_input(index);
        validate_input(&key, argument)?;
        with.insert(key, argument.clone());
    }
    for (key, value) in &parsed.env {
        let input = env_input(key);
        validate_input(&input, value)?;
        with.insert(input, value.clone());
    }
    Ok(Step {
        name: original.name.clone(),
        id: original.id,
        role: original.role,
        condition: original.condition.clone(),
        kind: StepKind::Action {
            uses,
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
    let mut inputs = Vec::new();
    for index in 0..shape.argv_count {
        inputs.push(input_definition(argv_input(index)));
    }
    for key in &shape.env_keys {
        inputs.push(input_definition(env_input(key)));
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
    crate::commands::validate_composite_env(&env)?;
    let script = task_script(shape.argv_count, &shape.helper);
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
    let body = composite::composite_yaml_with_inputs(
        &format!("{ACTION_NAME_PREFIX}{action_id}"),
        inputs,
        vec![step],
    )?;
    let bytes = marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{ACTION_NAME_PREFIX}{action_id}/action.yml"),
        bytes,
    })
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

fn task_script(argv_count: usize, helper: &str) -> String {
    let argv = (0..argv_count)
        .map(|index| format!("\"$VELNOR_WRAPPER_ARGV_{index}\""))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "unset {};\nset +e\nstarted_ms=$(date +%s%3N)\nargv=( {argv} )\n\"${{argv[@]}}\"\ntask_code=$?\nVELNOR_EXIT_CODE=\"$task_code\" VELNOR_START_MS=\"$started_ms\" VELNOR_INTERNAL_OP=write-task-report-v1 \"{helper}\"\nreport_code=$?\nif [ \"$task_code\" -ne 0 ]; then exit \"$task_code\"; fi\nexit \"$report_code\"",
        toolchain_env::CREDENTIAL_UNSET_VARS.join(" ")
    )
}

#[cfg(test)]
#[path = "task_wrapper_tests.rs"]
mod tests;
