// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, JobTimeout, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_GATE_CONDITION,
    REQUIRED_GATE_FAIL_CONDITION, REQUIRED_GATE_JOB_ID, REQUIRED_VERDICT_DISPLAY_NAME,
};

use crate::RenderError;
use crate::render::FINAL_JOB_ID;
use crate::yaml::Yaml;

const GATE_STEP_NAME: &str = "Fail unless the verdict passed";

pub(crate) fn insert_required_gate(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let Some(verdict) = jobs.get(FINAL_JOB_ID) else {
        return Ok(());
    };
    let gate = required_gate_job(&verdict.runs_on)?;
    crate::support::insert_support_job(jobs, REQUIRED_GATE_JOB_ID, gate, "required-gate")
}

fn required_gate_job(runs_on: &str) -> Result<Job, RenderError> {
    let mut step =
        crate::steps::shell_step(GATE_STEP_NAME, vec!["false".to_owned()], BTreeMap::new())?;
    step.condition = Some(REQUIRED_GATE_FAIL_CONDITION.to_owned());
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: REQUIRED_DISPLAY_NAME.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: vec![FINAL_JOB_ID.to_owned()],
        condition: Some(REQUIRED_GATE_CONDITION.to_owned()),
        permissions: None,
        cache_access: velnor_actions_contract::CacheAccess::Read,
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps: vec![step],
    })
}

pub(crate) fn required_gate_job_yaml(runs_on: &str) -> Result<(String, Yaml), RenderError> {
    let job = required_gate_job(runs_on)?;
    let verdict = Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: REQUIRED_VERDICT_DISPLAY_NAME.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: Vec::new(),
        condition: Some(REQUIRED_CONDITION.to_owned()),
        permissions: None,
        cache_access: velnor_actions_contract::CacheAccess::Read,
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps: Vec::new(),
    };
    let jobs = BTreeMap::from([
        (FINAL_JOB_ID.to_owned(), verdict),
        (REQUIRED_GATE_JOB_ID.to_owned(), job.clone()),
    ]);
    check_required_gate(&jobs)?;
    if job.check_runner.is_some()
        || job.permissions.is_some()
        || !job.cache_access.is_read()
        || job.environment.is_some()
        || !job.services.is_empty()
    {
        return Err(RenderError::InvalidWorkflow(
            "required_gate_render_policy".to_owned(),
        ));
    }
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if !job.needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(job.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(condition) = &job.condition {
        fields.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    let steps = job
        .steps
        .iter()
        .map(|step| {
            crate::document_steps::non_internal_step_to_yaml(
                REQUIRED_GATE_JOB_ID,
                step,
                &BTreeMap::new(),
                &job.runs_on,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    Ok((REQUIRED_GATE_JOB_ID.to_owned(), Yaml::Map(fields)))
}

fn gate_step() -> Result<velnor_actions_contract::Step, RenderError> {
    required_gate_job("ubuntu-26.04")?
        .steps
        .into_iter()
        .next()
        .ok_or_else(|| RenderError::InvalidWorkflow("required_gate_missing_step".to_owned()))
}

pub(crate) const REQUIRED_GATE_FORK_JOB_ID: &str = "required-gate-fork";

fn fenced_gate_condition() -> String {
    velnor_actions_contract::workflow::fork_fenced_condition(Some(REQUIRED_GATE_CONDITION))
}

fn fork_gate_condition() -> String {
    format!(
        "${{{{ always() && !({}) }}}}",
        velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION
    )
}

/// The gate moved to the scale set is skipped for fork pull requests, and a
/// skipped required check counts as passing. This hosted twin carries the
/// same check name and fails unless the verdict passed, so a fork pull
/// request fails closed instead of passing by skipping.
pub(crate) fn add_fork_gate(
    jobs: &mut BTreeMap<String, Job>,
    hosted: &str,
) -> Result<(), RenderError> {
    let mut twin = required_gate_job(hosted)?;
    twin.condition = Some(fork_gate_condition());
    crate::support::insert_support_job(jobs, REQUIRED_GATE_FORK_JOB_ID, twin, "required-gate")
}

pub(crate) fn check_required_gate(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if !jobs.contains_key(FINAL_JOB_ID) {
        return Ok(());
    }
    let bad = |problem: &str| Err(RenderError::InvalidWorkflow(problem.to_owned()));
    let Some(gate) = jobs.get(REQUIRED_GATE_JOB_ID) else {
        return bad("missing_required_gate");
    };
    let fenced = gate.condition.as_deref() == Some(fenced_gate_condition().as_str());
    if gate.display_name != REQUIRED_DISPLAY_NAME
        || (gate.condition.as_deref() != Some(REQUIRED_GATE_CONDITION) && !fenced)
        || gate.needs != [FINAL_JOB_ID]
        || gate.steps != [gate_step()?]
    {
        return bad("bad_required_gate");
    }
    if gate.check_runner.is_some()
        || gate.permissions.is_some()
        || !gate.cache_access.is_read()
        || gate.environment.is_some()
        || !gate.services.is_empty()
    {
        return bad("required_gate_carries_payload");
    }
    match (fenced, jobs.get(REQUIRED_GATE_FORK_JOB_ID)) {
        (true, Some(twin))
            if twin.display_name == REQUIRED_DISPLAY_NAME
                && twin.condition.as_deref() == Some(fork_gate_condition().as_str())
                && twin.needs == [FINAL_JOB_ID]
                && twin.steps == gate.steps
                && !twin.runs_on.starts_with("scale-set:") => {}
        (true, _) => return bad("fenced_required_gate_without_hosted_fork_twin"),
        (false, None) => {}
        (false, Some(_)) => return bad("unexpected_required_gate_fork_twin"),
    }
    if jobs.iter().any(|(id, job)| {
        id != REQUIRED_GATE_JOB_ID
            && id != REQUIRED_GATE_FORK_JOB_ID
            && job.display_name == REQUIRED_DISPLAY_NAME
    }) {
        return bad("duplicate_required_check_name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: &str = "scale-set:velnor+ubuntu-26.04-scale-set";

    fn verdict() -> Result<Job, RenderError> {
        let mut job = required_gate_job("ubuntu-26.04")?;
        job.display_name = REQUIRED_VERDICT_DISPLAY_NAME.to_owned();
        job.needs.clear();
        job.steps.clear();
        job.condition = Some(REQUIRED_CONDITION.to_owned());
        Ok(job)
    }

    fn fenced_gate() -> Result<Job, RenderError> {
        let mut gate = required_gate_job(SCALE)?;
        gate.condition = Some(fenced_gate_condition());
        Ok(gate)
    }

    fn jobs(with_twin: bool) -> Result<BTreeMap<String, Job>, RenderError> {
        let mut jobs = BTreeMap::from([
            (FINAL_JOB_ID.to_owned(), verdict()?),
            (REQUIRED_GATE_JOB_ID.to_owned(), fenced_gate()?),
        ]);
        if with_twin {
            add_fork_gate(&mut jobs, "ubuntu-26.04")?;
        }
        Ok(jobs)
    }

    #[test]
    fn a_fenced_gate_without_the_hosted_fork_twin_is_rejected() -> Result<(), RenderError> {
        let error = check_required_gate(&jobs(false)?)
            .err()
            .map(|e| e.to_string());
        assert!(error.is_some_and(|e| e.contains("fenced_required_gate_without_hosted_fork_twin")));
        Ok(())
    }

    #[test]
    fn the_hosted_fork_twin_fails_closed_for_fork_pull_requests() -> Result<(), RenderError> {
        let jobs = jobs(true)?;
        check_required_gate(&jobs)?;
        let twin = jobs
            .get(REQUIRED_GATE_FORK_JOB_ID)
            .ok_or_else(|| RenderError::InvalidWorkflow("twin missing".to_owned()))?;
        assert_eq!(twin.runs_on, "ubuntu-26.04");
        assert_eq!(twin.display_name, REQUIRED_DISPLAY_NAME);
        let condition = twin.condition.clone().unwrap_or_default();
        assert!(condition.contains("&& !(github.event_name == 'push'"));
        // The twin runs the gate step, which fails unless the verdict job succeeded.
        assert_eq!(twin.needs, [FINAL_JOB_ID]);
        assert_eq!(twin.steps, jobs[REQUIRED_GATE_JOB_ID].steps);
        assert_eq!(
            twin.steps[0].condition.as_deref(),
            Some(REQUIRED_GATE_FAIL_CONDITION)
        );
        Ok(())
    }

    #[test]
    fn a_scale_set_twin_is_rejected() -> Result<(), RenderError> {
        let mut jobs = jobs(true)?;
        if let Some(twin) = jobs.get_mut(REQUIRED_GATE_FORK_JOB_ID) {
            SCALE.clone_into(&mut twin.runs_on);
        }
        assert!(check_required_gate(&jobs).is_err());
        Ok(())
    }

    #[test]
    fn an_unfenced_gate_needs_no_twin() -> Result<(), RenderError> {
        let jobs = BTreeMap::from([
            (FINAL_JOB_ID.to_owned(), verdict()?),
            (
                REQUIRED_GATE_JOB_ID.to_owned(),
                required_gate_job("ubuntu-26.04")?,
            ),
        ]);
        check_required_gate(&jobs)
    }
}

#[cfg(test)]
#[path = "required_gate_fork_tests.rs"]
mod fork_tests;
