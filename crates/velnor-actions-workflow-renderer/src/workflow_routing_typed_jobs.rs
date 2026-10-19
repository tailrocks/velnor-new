use velnor_actions_contract::{
    CheckExecutor, CheckPlatform, Job, RunnerPlatform, RunsOn,
    config::EPHEMERAL_CHECK_ADMISSION_CONDITION, runner_platform,
};

use super::{WorkflowRoutingPolicy, invalid, labels};
use crate::RenderError;

/// Apply the selected runner placement after policy jobs have been merged.
///
/// Keeping this on the typed job map covers support jobs added after the
/// user's workflow intermediate representation, including repository lint jobs.
pub(crate) fn route_jobs(
    jobs: &mut std::collections::BTreeMap<String, Job>,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let selector = policy.selector();
    for (job_id, job) in jobs {
        let parsed = RunsOn::parse(&job.runs_on).map_err(|_| {
            invalid(format!(
                "unsupported_runs_on_for_linux_amd64_scale_set:.github/workflows/ci.yml:{job_id}:{}",
                job.runs_on
            ))
        })?;
        if let Some(runner) = job
            .check_runner
            .as_ref()
            .filter(|runner| runner.executor == CheckExecutor::EphemeralSelfHosted)
        {
            if runner.platform == CheckPlatform::LinuxX64
                && policy.selector().is_some_and(
                    |selector| matches!(&parsed, RunsOn::ScaleSet(current) if current == selector),
                )
                && is_valid_typed_scale_set_check(job_id, job)
            {
                continue;
            }
            if policy.mode != velnor_actions_contract::ExecutionMode::ScaleSet
                && is_valid_typed_native_check(job_id, job)
            {
                continue;
            }
            return Err(invalid(format!(
                "typed_ephemeral_check_policy_invalid:.github/workflows/ci.yml:{job_id}"
            )));
        }
        match (selector, parsed) {
            (None, RunsOn::ScaleSet(_)) => {
                return Err(invalid(format!(
                    "scale_set_selector_unconfigured:.github/workflows/ci.yml:{job_id}"
                )));
            }
            (Some(selector), RunsOn::ScaleSet(existing)) if existing == *selector => {}
            (Some(_), RunsOn::ScaleSet(_)) => {
                return Err(invalid(format!(
                    "scale_set_selector_mismatch:.github/workflows/ci.yml:{job_id}"
                )));
            }
            (Some(selector), RunsOn::Hosted(label)) if policy.is_scale_set() => {
                match runner_platform(&label) {
                    Some(RunnerPlatform::Darwin) => {
                        return Err(invalid(format!(
                            "unsupported_darwin_workload:.github/workflows/ci.yml:{job_id}:{label}"
                        )));
                    }
                    Some(RunnerPlatform::LinuxAmd64) => job.runs_on = selector.token(),
                    Some(RunnerPlatform::LinuxArm64) | None => {
                        return Err(invalid(format!(
                            "unsupported_workload_for_linux_amd64_scale_set:.github/workflows/ci.yml:{job_id}:{label}"
                        )));
                    }
                }
            }
            (_, RunsOn::Hosted(label)) => {
                labels::validate_preserved_hosted_labels(
                    std::slice::from_ref(&label),
                    ".github/workflows/ci.yml",
                    job_id,
                )?;
            }
        }
    }
    Ok(())
}

fn is_valid_typed_check(job_id: &str, job: &Job, require_runner_label: bool) -> bool {
    let Some(runner) = &job.check_runner else {
        return false;
    };
    if runner.executor != CheckExecutor::EphemeralSelfHosted
        || (require_runner_label && runner.label != job.runs_on)
        || !job_id.starts_with("check-")
        || runner.validate(".github/workflows/ci.yml", job_id).is_err()
    {
        return false;
    }
    job.condition.as_deref().is_some_and(|condition| {
        condition == EPHEMERAL_CHECK_ADMISSION_CONDITION
            || condition
                == "always() && needs.plan.result == 'success' && ((needs.plan.outputs.qualification_campaign == '' && (github.event_name == 'push' || github.event_name == 'merge_group' || (github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository))) || needs.plan.outputs.qualification_campaign != '')"
    })
}

fn is_valid_typed_native_check(job_id: &str, job: &Job) -> bool {
    is_valid_typed_check(job_id, job, true)
}

fn is_valid_typed_scale_set_check(job_id: &str, job: &Job) -> bool {
    let Some(runner) = &job.check_runner else {
        return false;
    };
    runner.platform == CheckPlatform::LinuxX64 && is_valid_typed_check(job_id, job, false)
}

pub(super) fn accept_typed_native_check(
    runs_on: Option<&str>,
    condition: Option<&str>,
    has_container: bool,
    typed_job: Option<&Job>,
    policy: &WorkflowRoutingPolicy,
    path: &str,
    job_id: &str,
) -> Result<bool, RenderError> {
    let Some(job) = typed_job.filter(|job| {
        job.check_runner.as_ref().is_some_and(|runner| {
            runner.executor == CheckExecutor::EphemeralSelfHosted && runner.label == job.runs_on
        })
    }) else {
        return Ok(false);
    };
    let Some(runner) = job.check_runner.as_ref() else {
        return Ok(false);
    };
    if policy.mode == velnor_actions_contract::ExecutionMode::ScaleSet
        || path != ".github/workflows/ci.yml"
        || !is_valid_typed_native_check(job_id, job)
        || runs_on != Some(runner.label.as_str())
        || condition != job.condition.as_deref()
        || has_container
    {
        return Err(invalid(format!(
            "typed_ephemeral_check_policy_invalid:{path}:{job_id}"
        )));
    }
    Ok(true)
}
