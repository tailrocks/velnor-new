//! Typed marker for plans whose matrix becomes a GitHub job output.

use crate::matrix::matrix_invalid;
use velnor_actions_contract_workflow::{DYNAMIC_MATRIX_OUTPUT_MODE, PLAN_MATRIX_OUTPUT_MODE_ENV};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

/// Mark the plan step when its matrix is promoted to a job output.
pub(crate) fn mark_dynamic_matrix_output_mode(
    jobs: &mut [(String, Yaml)],
    job_id: &str,
    step_id: &str,
) -> Result<(), RenderError> {
    let steps = jobs
        .iter_mut()
        .find_map(|(name, job)| (name == job_id).then_some(job))
        .and_then(|job| match job {
            Yaml::Map(entries) => entries
                .iter_mut()
                .find_map(|(name, value)| (name == "steps").then_some(value)),
            _ => None,
        });
    let Some(Yaml::Seq(items)) = steps else {
        return Err(matrix_invalid("matrix_plan_steps_missing"));
    };
    let Some(step) = items.iter_mut().find_map(|item| match item {
        Yaml::Map(entries)
            if entries.iter().any(|(name, value)| {
                name == "id" && matches!(value, Yaml::Str(id) if id == step_id)
            }) =>
        {
            Some(entries)
        }
        _ => None,
    }) else {
        return Err(matrix_invalid("matrix_plan_step_id_missing"));
    };
    let Some(Yaml::Map(env)) = step
        .iter_mut()
        .find_map(|(name, value)| (name == "env").then_some(value))
    else {
        return Err(matrix_invalid("matrix_plan_step_env_missing"));
    };
    if let Some((_, value)) = env
        .iter()
        .find(|(name, _)| name == PLAN_MATRIX_OUTPUT_MODE_ENV)
    {
        if value == &Yaml::str(DYNAMIC_MATRIX_OUTPUT_MODE.to_owned()) {
            return Ok(());
        }
        return Err(matrix_invalid("matrix_plan_output_mode_conflict"));
    }
    env.push((
        PLAN_MATRIX_OUTPUT_MODE_ENV.to_owned(),
        Yaml::str(DYNAMIC_MATRIX_OUTPUT_MODE.to_owned()),
    ));
    env.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(())
}
