//! Collision, candidate-isolation, and final-gate invariants for support jobs.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, StepKind};
use velnor_actions_workflow_steps::{RenderError, steps};

use crate::context::{
    CANDIDATE_JOB_ID, FINAL_CONDITION, FINAL_DISPLAY_NAME, FINAL_JOB_ID, PLAN_JOB_ID,
};

use super::{LINT_DISPLAY_NAME, LINT_JOB_ID, RELEASE_JOB_ID};

/// Insert a support job, failing on ID collision with IR jobs.
pub(crate) fn insert_support_job(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    job: Job,
) -> Result<(), RenderError> {
    if jobs.contains_key(id) {
        return Err(RenderError::PolicyRejected {
            policy: "velnor-repository-v1".to_owned(),
            problem: format!("job_collision:{id}"),
        });
    }
    jobs.insert(id.to_owned(), job);
    Ok(())
}

/// Candidate never plans: it needs plan, holds no plan step, feeds no task.
///
/// Qualification downloads the built artifact; a candidate without a
/// download step cannot prove the no-rebuild path and is rejected.
pub(crate) fn check_candidate_invariants(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if let Some(candidate) = jobs.get(CANDIDATE_JOB_ID) {
        if !candidate.needs.contains(&PLAN_JOB_ID.to_owned()) {
            return Err(RenderError::InvalidWorkflow(
                "candidate_must_need_plan".to_owned(),
            ));
        }
        for step in &candidate.steps {
            if let StepKind::Internal { operation, .. } = &step.kind
                && operation == steps::PLAN_OPERATION
            {
                return Err(RenderError::InvalidWorkflow(
                    "candidate_must_not_plan".to_owned(),
                ));
            }
        }
        let downloaded = candidate.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES)
        });
        if !downloaded {
            return Err(RenderError::InvalidWorkflow(
                "candidate_must_download_artifact".to_owned(),
            ));
        }
    }
    for (id, job) in jobs {
        if id.as_str() != FINAL_JOB_ID
            && id.as_str() != RELEASE_JOB_ID
            && job.needs.contains(&CANDIDATE_JOB_ID.to_owned())
        {
            return Err(RenderError::InvalidWorkflow(
                "task_must_not_consume_candidate".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Final gate keeps its required name/condition and the always-on lint name.
pub(crate) fn check_final_gate(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if let Some(final_job) = jobs.get(FINAL_JOB_ID) {
        if final_job.display_name != FINAL_DISPLAY_NAME {
            return Err(RenderError::InvalidWorkflow("bad_final_name".to_owned()));
        }
        if final_job.condition.as_deref() != Some(FINAL_CONDITION) {
            return Err(RenderError::InvalidWorkflow(
                "bad_final_condition".to_owned(),
            ));
        }
    }
    if let Some(lint) = jobs.get(LINT_JOB_ID)
        && lint.display_name != LINT_DISPLAY_NAME
    {
        return Err(RenderError::InvalidWorkflow("bad_lint_name".to_owned()));
    }
    Ok(())
}
