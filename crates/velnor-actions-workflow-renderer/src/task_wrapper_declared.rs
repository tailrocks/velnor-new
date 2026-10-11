use super::{
    ACTION_NAME_PREFIX, BTreeMap, DECLARED_TASK_ACTION_PREFIX, EXECUTION_DIGEST_INPUT,
    GENERATOR_VERSION_ENV, RUNNER_TEMP_EXPRESSION, RUNTIME_RUNNER_TEMP_ENV, RenderError,
    RenderedFile, Shape, Step, StepKind, TASK_EXECUTION_DIGEST_ENV, TaskExecutionRef, Yaml,
    composite, marker, steps, task_script,
};

use crate::toolchain_env;

pub(super) fn validate_task_fields(
    argv: &[String],
    env: &BTreeMap<String, String>,
    task_id: &str,
    task_digest: &str,
    matrix_id: &str,
    matrix_key: &str,
) -> Result<(), RenderError> {
    for value in argv.iter().chain(env.values()) {
        validate_runtime_expression(value)?;
    }
    crate::commands::validate_command_argv(argv)?;
    crate::commands::validate_env(env)?;
    crate::toolchain_env::reject_denied_step_keys(env)?;
    for key in env.keys() {
        if matches!(
            key.as_str(),
            "VELNOR_TASK_ID"
                | "VELNOR_TASK_DIGEST"
                | "VELNOR_MATRIX_ID"
                | "VELNOR_MATRIX_KEY"
                | "VELNOR_INTERNAL_OP"
                | "VELNOR_GENERATOR_VERSION"
                | "VELNOR_TASK_EXECUTION_DIGEST"
                | "VELNOR_RUNTIME_RUNNER_TEMP"
        ) {
            return Err(RenderError::BadCommand(format!(
                "reserved_declared_task_env_key:{key}"
            )));
        }
    }
    for (index, argument) in argv.iter().enumerate() {
        validate_input(&format!("argv_{index}"), argument)?;
    }
    for (key, value) in env {
        validate_input(&format!("env_{key}"), value)?;
    }
    for (key, value) in [
        ("task_id", task_id),
        ("task_digest", task_digest),
        ("matrix_id", matrix_id),
        ("matrix_key", matrix_key),
    ] {
        validate_input(key, value)?;
    }
    Ok(())
}

pub(super) fn declared_task_call(
    action_id: usize,
    task: &TaskExecutionRef<'_>,
    original: &Step,
) -> Step {
    let with = BTreeMap::from([(
        EXECUTION_DIGEST_INPUT.to_owned(),
        task.execution_digest.clone(),
    )]);
    Step {
        name: original.name.clone(),
        id: original.id,
        role: original.role,
        condition: original.condition.clone(),
        kind: StepKind::Action {
            uses: format!("{DECLARED_TASK_ACTION_PREFIX}{action_id}"),
            with,
            env: BTreeMap::new(),
        },
    }
}

/// Task manifests currently carry only the exact GitHub expression
/// `${{ runner.temp }}`. The data-only resolver materializes that value after
/// checking the manifest and plan; other expressions fail before serialization.
fn validate_runtime_expression(value: &str) -> Result<(), RenderError> {
    let mut rest = value;
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else {
            return Err(RenderError::BadCommand(
                "unclosed_declared_task_expression".to_owned(),
            ));
        };
        if &after[..end] != " runner.temp " {
            return Err(RenderError::BadCommand(
                "unsupported_declared_task_expression".to_owned(),
            ));
        }
        rest = &after[end + 2..];
    }
    Ok(())
}

fn validate_input(key: &str, value: &str) -> Result<(), RenderError> {
    crate::expressions::check_with_key(key)?;
    crate::expressions::check_with_value(key, value)?;
    steps::scan_for_private_subcommands(key)?;
    steps::scan_for_private_subcommands(value)
}

pub(super) fn declared_task_file(
    action_id: usize,
    shape: &Shape,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    let body = declared_task_document(action_id, shape, version)?;
    let bytes = marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{ACTION_NAME_PREFIX}{action_id}/action.yml"),
        bytes,
    })
}

pub(super) fn declared_task_document(
    action_id: usize,
    shape: &Shape,
    generator_version: &str,
) -> Result<Yaml, RenderError> {
    let inputs = vec![input_definition(EXECUTION_DIGEST_INPUT.to_owned())];
    let mut env = toolchain_env::credential_scrub();
    env.insert(
        TASK_EXECUTION_DIGEST_ENV.to_owned(),
        format!("${{{{ inputs.{EXECUTION_DIGEST_INPUT} }}}}"),
    );
    env.insert(
        GENERATOR_VERSION_ENV.to_owned(),
        generator_version.to_owned(),
    );
    env.insert(
        RUNTIME_RUNNER_TEMP_ENV.to_owned(),
        RUNNER_TEMP_EXPRESSION.to_owned(),
    );
    crate::commands::validate_composite_env(&env)?;
    let script = task_script(&shape.helper_version);
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
                Yaml::str("Full execution selection digest for one validated task record."),
            ),
            ("required".to_owned(), Yaml::Bool(true)),
        ]),
    )
}
