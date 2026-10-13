//! Task matrix strategy: marker trio plus `strategy`/`outputs` emission.
//!
//! The orchestrator marks the matrix consumer step with a fixed env trio;
//! the renderer validates the producer wiring, scrubs the markers, and
//! emits the typed `strategy` (`fail-fast: false`, capped `max-parallel`,
//! exact `fromJSON` producer ref) plus producer `outputs` and step ID.
//! Capped crate jobs carry the same trio on one obligation step; the
//! renderer validates it identically and emits the declared
//! `strategy.max-parallel` the lane staging enforces through `needs`.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, StepId, StepKind};

use crate::matrix_output_mode::mark_dynamic_matrix_output_mode;
use velnor_actions_workflow_jobs::context::{PLAN_JOB_ID, TASK_JOB_ID};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

/// Matrix marker: producer job backing `needs.<job>.outputs.<output>`.
pub const MATRIX_NEEDS_JOB_ENV: &str = "VELNOR_MATRIX_NEEDS_JOB";
/// Matrix marker: producer output name consumed via `fromJSON`.
pub const MATRIX_OUTPUT_ENV: &str = "VELNOR_MATRIX_OUTPUT";
/// Matrix marker: `strategy.max-parallel` entry cap.
pub const MATRIX_MAX_PARALLEL_ENV: &str = "VELNOR_MATRIX_MAX_PARALLEL";
/// Step ID of the matrix-producing plan step.
pub const PLAN_STEP_ID: &str = StepId::Plan.as_str();
/// Job-output name carrying the plan ID (`plan-<run-key>`).
pub const PLAN_ID_OUTPUT: &str = "plan_id";
/// Job-output name carrying the run key (`r<run-id>-a<run-attempt>`).
pub const RUN_KEY_OUTPUT: &str = "run_key";
/// Job-output name carrying the comma-wrapped covered task IDs.
pub const COVERED_TASKS_OUTPUT: &str = "covered_tasks";

/// Typed matrix source for the task job's `strategy.matrix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixSource {
    /// `strategy.matrix` from `fromJSON(needs.<job>.outputs.<output>)`.
    PlanOutput {
        /// Producer job ID the task job needs.
        needs_job: String,
        /// Producer output name carrying the matrix JSON.
        output: String,
    },
}

/// Shorthand for a matrix invariant failure.
pub(crate) fn matrix_invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(problem.to_owned())
}

/// First marker trio hit in one job's shell steps, if any key present.
fn marker_trio(job: &Job) -> Option<[Option<String>; 3]> {
    for step in &job.steps {
        if let StepKind::Shell { env, .. } = &step.kind {
            let hit = [
                MATRIX_NEEDS_JOB_ENV,
                MATRIX_OUTPUT_ENV,
                MATRIX_MAX_PARALLEL_ENV,
            ]
            .map(|key| env.get(key).cloned());
            if hit.iter().any(Option::is_some) {
                return Some(hit);
            }
        }
    }
    None
}

/// Task matrix directive from the fixed-step env trio (`None` = static).
pub(crate) fn task_matrix_of(
    jobs: &BTreeMap<String, Job>,
) -> Result<Option<(MatrixSource, u32)>, RenderError> {
    let Some(task) = jobs.get(TASK_JOB_ID) else {
        return Ok(None);
    };
    let trio = marker_trio(task);
    let Some([Some(job), Some(out), Some(max)]) = trio else {
        return if trio.is_some() {
            Err(matrix_invalid("matrix_marker_partial"))
        } else {
            Ok(None)
        };
    };
    if !is_matrix_name(&job) || !is_matrix_name(&out) {
        return Err(matrix_invalid("matrix_bad_producer"));
    }
    let max = max
        .parse::<u32>()
        .ok()
        .filter(|max| *max >= 1)
        .ok_or_else(|| matrix_invalid("matrix_bad_max_parallel"))?;
    if !jobs.contains_key(job.as_str()) {
        return Err(matrix_invalid("matrix_without_producer_job"));
    }
    if !task.needs.contains(&job) {
        return Err(matrix_invalid("matrix_without_producer_need"));
    }
    Ok(Some((
        MatrixSource::PlanOutput {
            needs_job: job.clone(),
            output: out.clone(),
        },
        max,
    )))
}

/// Declared concurrency caps from per-job marker trios, by job ID.
///
/// Every non-task job carrying any trio key must carry the full trio
/// with a live producer and a positive cap, exactly like the task
/// matrix; unmarked jobs stay out.
pub(crate) fn crate_job_caps(
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeMap<String, u32>, RenderError> {
    let mut caps = BTreeMap::new();
    for (id, job) in jobs {
        if id == TASK_JOB_ID {
            continue;
        }
        let trio = marker_trio(job);
        let Some([Some(producer), Some(output), Some(max)]) = trio else {
            if trio.is_some() {
                return Err(matrix_invalid("matrix_marker_partial"));
            }
            continue;
        };
        if !is_matrix_name(&producer) || !is_matrix_name(&output) {
            return Err(matrix_invalid("matrix_bad_producer"));
        }
        let max = max
            .parse::<u32>()
            .ok()
            .filter(|max| *max >= 1)
            .ok_or_else(|| matrix_invalid("matrix_bad_max_parallel"))?;
        if !jobs.contains_key(producer.as_str()) {
            return Err(matrix_invalid("matrix_without_producer_job"));
        }
        if !job.needs.contains(&producer) {
            return Err(matrix_invalid("matrix_without_producer_need"));
        }
        caps.insert(id.clone(), max);
    }
    Ok(caps)
}

/// Clone jobs minus the matrix marker trio, which never renders.
pub(crate) fn scrub_matrix_marker(jobs: &BTreeMap<String, Job>) -> BTreeMap<String, Job> {
    let mut scrubbed = jobs.clone();
    for job in scrubbed.values_mut() {
        for step in &mut job.steps {
            if let StepKind::Shell { env, .. } = &mut step.kind {
                for key in [
                    MATRIX_NEEDS_JOB_ENV,
                    MATRIX_OUTPUT_ENV,
                    MATRIX_MAX_PARALLEL_ENV,
                ] {
                    env.remove(key);
                }
            }
        }
    }
    scrubbed
}

/// True for `needs.<job>.outputs.<output>` identifier spellings.
fn is_matrix_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Emit the task `strategy`, producer `outputs`, and plan step ID.
pub(crate) fn attach_task_matrix(
    document: &mut Yaml,
    source: &MatrixSource,
    max_parallel: u32,
) -> Result<(), RenderError> {
    let MatrixSource::PlanOutput { needs_job, output } = source;
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
    insert_job_key(
        jobs,
        TASK_JOB_ID,
        "strategy",
        Yaml::Map(vec![
            ("fail-fast".to_owned(), Yaml::Bool(false)),
            ("max-parallel".to_owned(), Yaml::Int(max_parallel.into())),
            (
                "matrix".to_owned(),
                Yaml::str(format!(
                    "${{{{ fromJSON(needs.{needs_job}.outputs.{output}) }}}}"
                )),
            ),
        ]),
    )?;
    let mut outputs = vec![(
        output.clone(),
        Yaml::str(format!("${{{{ steps.{PLAN_STEP_ID}.outputs.{output} }}}}")),
    )];
    for name in [PLAN_ID_OUTPUT, RUN_KEY_OUTPUT, COVERED_TASKS_OUTPUT] {
        if name != output {
            outputs.push((
                name.to_owned(),
                Yaml::str(format!("${{{{ steps.{PLAN_STEP_ID}.outputs.{name} }}}}")),
            ));
        }
    }
    insert_job_key(jobs, needs_job, "outputs", Yaml::Map(outputs))?;
    require_plan_step_id(jobs, needs_job, output)
}

/// Emit each capped job's declared `strategy.max-parallel`.
///
/// The cap declares the lane-staging bound the orchestrator enforces
/// through `needs`; without it the staging would be invisible. Empty
/// caps change nothing.
pub(crate) fn attach_crate_job_caps(
    document: &mut Yaml,
    caps: &BTreeMap<String, u32>,
) -> Result<(), RenderError> {
    if caps.is_empty() {
        return Ok(());
    }
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
    for (id, cap) in caps {
        insert_job_key(
            jobs,
            id,
            "strategy",
            Yaml::Map(vec![("max-parallel".to_owned(), Yaml::Int((*cap).into()))]),
        )?;
    }
    Ok(())
}

/// Emit the plan job's `covered_tasks` output plus the plan step ID.
///
/// Static (non-matrix) workflows still gate obligation steps on plan
/// coverage, so the plan job always exports the covered set; without
/// a plan job or plan step there is nothing to export.
pub(crate) fn attach_plan_outputs(document: &mut Yaml) -> Result<(), RenderError> {
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
    if !has_step_id(jobs, PLAN_JOB_ID, PLAN_STEP_ID) {
        return Ok(());
    }
    insert_job_key(
        jobs,
        PLAN_JOB_ID,
        "outputs",
        Yaml::Map(vec![(
            COVERED_TASKS_OUTPUT.to_owned(),
            Yaml::str(format!(
                "${{{{ steps.{PLAN_STEP_ID}.outputs.{COVERED_TASKS_OUTPUT} }}}}"
            )),
        )]),
    )
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
