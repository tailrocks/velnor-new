/// Tag the publish job's publish step with its step ID.
///
/// The upload step names the artifact through the publish step's
/// outputs; without the tag the reference dangles. No-op without a
/// publish job.
pub(crate) fn insert_publish_step_id(document: &mut Yaml) -> Result<(), RenderError> {
    let Yaml::Map(entries) = document else {
        return Err(matrix_invalid("matrix_without_document"));
    };
    let Some(Yaml::Map(jobs)) = entries
        .iter_mut()
        .find(|entry| entry.0 == "jobs")
        .map(|entry| &mut entry.1)
    else {
        return Err(matrix_invalid("matrix_without_jobs"));
    };
    tag_op_step(
        jobs,
        PUBLISH_JOB_ID,
        steps::PUBLISH_OPERATION,
        steps::PUBLISH_STEP_ID,
    );
    Ok(())
}

fn tag_op_step(jobs: &mut [(String, Yaml)], id: &str, operation: &str, step_id: &str) -> bool {
    let steps = jobs
        .iter_mut()
        .find_map(|(name, job)| (name == id).then_some(job))
        .and_then(|job| {
            if let Yaml::Map(entries) = job {
                entries
                    .iter_mut()
                    .find_map(|(name, value)| (name == "steps").then_some(value))
            } else {
                None
            }
        });
    let Some(Yaml::Seq(items)) = steps else {
        return false;
    };
    for item in items {
        let Yaml::Map(step) = item else { continue };
        let targeted = step.iter().any(|(name, env)| {
            name == "env"
                && matches!(env, Yaml::Map(vars) if vars.iter().any(|(var, val)| {
                    var == steps::INTERNAL_OP_ENV && matches!(val, Yaml::Str(op) if op == operation)
                }))
        });
        if targeted {
            step.insert(1, ("id".to_owned(), Yaml::str(step_id.to_owned())));
            return true;
        }
    }
    false
}

/// Insert a job key directly before its `steps` entry.
fn insert_job_key(
    jobs: &mut [(String, Yaml)],
    id: &str,
    key: &str,
    value: Yaml,
) -> Result<(), RenderError> {
    let Some(entries) = jobs.iter_mut().find_map(|(name, job)| {
        if name == id
            && let Yaml::Map(entries) = job
        {
            return Some(entries);
        }
        None
    }) else {
        return Err(matrix_invalid(&format!("matrix_without_job:{id}")));
    };
    let Some(index) = entries.iter().position(|entry| entry.0 == "steps") else {
        return Err(matrix_invalid(&format!("matrix_without_steps:{key}")));
    };
    entries.insert(index, (key.to_owned(), value));
    Ok(())
}

/// Require the planner's typed output id before wiring job outputs.
fn require_plan_step_id(
    jobs: &mut [(String, Yaml)],
    id: &str,
    output: &str,
) -> Result<(), RenderError> {
    if !has_step_id(jobs, id, PLAN_STEP_ID) {
        return Err(matrix_invalid(&format!(
            "matrix_without_plan_step:{output}"
        )));
    }
    mark_dynamic_matrix_output_mode(jobs, id, PLAN_STEP_ID)
}


/// Check for one explicitly declared step id in a serialized job.
fn has_step_id(jobs: &[(String, Yaml)], id: &str, step_id: &str) -> bool {
    let steps = jobs
        .iter()
        .find_map(|(name, job)| (name == id).then_some(job))
        .and_then(|job| {
            if let Yaml::Map(entries) = job {
                entries
                    .iter()
                    .find_map(|(name, value)| (name == "steps").then_some(value))
            } else {
                None
            }
        });
    let Some(Yaml::Seq(items)) = steps else {
        return false;
    };
    items.iter().any(|item| {
        matches!(
            item,
            Yaml::Map(step) if step.iter().any(|(name, value)| {
                name == "id" && matches!(value, Yaml::Str(actual) if actual == step_id)
            })
        )
    })
}
