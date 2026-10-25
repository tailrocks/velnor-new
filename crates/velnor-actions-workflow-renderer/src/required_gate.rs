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
        display_name: REQUIRED_DISPLAY_NAME.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: vec![FINAL_JOB_ID.to_owned()],
        condition: Some(REQUIRED_GATE_CONDITION.to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: vec![step],
    })
}

pub(crate) fn required_gate_job_yaml(runs_on: &str) -> Result<(String, Yaml), RenderError> {
    let job = required_gate_job(runs_on)?;
    let verdict = Job {
        services: Vec::new(),
        display_name: REQUIRED_VERDICT_DISPLAY_NAME.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: Vec::new(),
        condition: Some(REQUIRED_CONDITION.to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: Vec::new(),
    };
    let jobs = BTreeMap::from([
        (FINAL_JOB_ID.to_owned(), verdict),
        (REQUIRED_GATE_JOB_ID.to_owned(), job.clone()),
    ]);
    check_required_gate(&jobs)?;
    if job.check_runner.is_some()
        || job.permissions.is_some()
        || job.cache_mode.is_some()
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
    required_gate_job("ubuntu-latest")?
        .steps
        .into_iter()
        .next()
        .ok_or_else(|| RenderError::InvalidWorkflow("required_gate_missing_step".to_owned()))
}

pub(crate) fn check_required_gate(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if !jobs.contains_key(FINAL_JOB_ID) {
        return Ok(());
    }
    let bad = |problem: &str| Err(RenderError::InvalidWorkflow(problem.to_owned()));
    let Some(gate) = jobs.get(REQUIRED_GATE_JOB_ID) else {
        return bad("missing_required_gate");
    };
    if gate.display_name != REQUIRED_DISPLAY_NAME
        || gate.condition.as_deref() != Some(REQUIRED_GATE_CONDITION)
        || gate.needs != [FINAL_JOB_ID]
        || gate.steps != [gate_step()?]
    {
        return bad("bad_required_gate");
    }
    if jobs
        .iter()
        .any(|(id, job)| id != REQUIRED_GATE_JOB_ID && job.display_name == REQUIRED_DISPLAY_NAME)
    {
        return bad("duplicate_required_check_name");
    }
    Ok(())
}
