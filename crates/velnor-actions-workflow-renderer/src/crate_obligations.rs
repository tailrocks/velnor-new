//! Crate-job body extraction for workflows over GitHub's file cap.
//!
//! Inlined obligation commands are unique per crate, so YAML anchors cannot
//! share them. When `ci.yml` would exceed 500 KB, those steps move into one
//! local composite per crate job. The job keeps checkout, tool setup, and
//! report upload, and passes covered-task ids in, because composite steps
//! cannot read `needs`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step};

use crate::lane_share::LaneShare;
use crate::matrix::COVERED_TASKS_OUTPUT;
use crate::render::{PLAN_JOB_ID, RenderContext};
use crate::tree::RenderedFile;
use crate::{RenderError, composite, marker, steps, yaml};

/// Move obligation steps out of crate jobs that are still inlined.
///
/// Lane-shared jobs already keep their bodies in a composite. Returns the
/// number of actions written. Zero means this pass cannot shrink the file.
///
/// # Errors
///
/// Returns [`RenderError`] when an obligation block is split, its skip
/// condition is not the covered-task gate, or the action path collides.
pub(crate) fn extract_obligation_composites(
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<usize, RenderError> {
    let ids: Vec<String> = shared
        .jobs
        .keys()
        .filter(|id| !shared.calls.contains_key(*id))
        .cloned()
        .collect();
    let mut files = Vec::new();
    for id in ids {
        let Some(job) = shared.jobs.get_mut(&id) else {
            continue;
        };
        if let Some(file) = extract_job(&id, job, ctx)? {
            if shared
                .files
                .iter()
                .any(|existing| existing.path == file.path)
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "crate_action_collision:{}",
                    file.path
                )));
            }
            files.push(file);
        }
    }
    let count = files.len();
    shared.files.extend(files);
    Ok(count)
}

fn extract_job(
    job_id: &str,
    job: &mut Job,
    ctx: &RenderContext,
) -> Result<Option<RenderedFile>, RenderError> {
    let Some(start) = job.steps.iter().position(is_obligation_step) else {
        return Ok(None);
    };
    let end = job
        .steps
        .iter()
        .rposition(is_obligation_step)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("crate_obligation_missing:{job_id}"))
        })?;
    if job.steps[start..=end]
        .iter()
        .any(|step| !is_obligation_step(step))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "crate_obligations_split:{job_id}"
        )));
    }
    let needs = covered_needs_expr();
    let input = covered_input_expr();
    let mut obligations = job.steps[start..=end].to_vec();
    for step in &mut obligations {
        let Some(condition) = &mut step.condition else {
            return Err(RenderError::InvalidWorkflow(format!(
                "crate_obligation_condition:{job_id}"
            )));
        };
        if !condition.contains(&needs) {
            return Err(RenderError::InvalidWorkflow(format!(
                "crate_obligation_condition:{job_id}"
            )));
        }
        *condition = condition.replace(&needs, &input);
    }
    let file = obligation_action(job_id, &obligations, ctx)?;
    let call = steps::action_step(
        "Run crate obligations",
        &format!("./.github/actions/{job_id}"),
        BTreeMap::from([(COVERED_TASKS_OUTPUT.to_owned(), format!("${{{{{needs}}}}}"))]),
    )?;
    let mut steps = Vec::with_capacity(job.steps.len() - obligations.len() + 1);
    steps.extend(job.steps[..start].iter().cloned());
    steps.push(call);
    steps.extend(job.steps[end + 1..].iter().cloned());
    job.steps = steps;
    Ok(Some(file))
}

fn is_obligation_step(step: &Step) -> bool {
    step.condition
        .as_deref()
        .is_some_and(|condition| condition.contains(&covered_needs_expr()))
}

fn covered_needs_expr() -> String {
    format!("needs.{PLAN_JOB_ID}.outputs.{COVERED_TASKS_OUTPUT}")
}

fn covered_input_expr() -> String {
    format!("inputs.{COVERED_TASKS_OUTPUT}")
}

fn obligation_action(
    job_id: &str,
    obligations: &[Step],
    ctx: &RenderContext,
) -> Result<RenderedFile, RenderError> {
    velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(
        obligations,
        &format!("composite:{job_id}"),
    )
    .map_err(RenderError::Contract)?;
    let mut rendered = Vec::with_capacity(obligations.len());
    let empty_job_env = BTreeMap::new();
    let step_context = crate::document_lanes::JobStepContext {
        job_env: &empty_job_env,
        runs_on: None,
        actions_read: false,
    };
    for step in obligations {
        rendered.push(crate::document_steps::step_to_yaml(
            job_id,
            step,
            ctx,
            &[],
            true,
            &step_context,
        )?);
    }
    let inputs = vec![(
        COVERED_TASKS_OUTPUT.to_owned(),
        yaml::Yaml::Map(vec![
            (
                "description".to_owned(),
                yaml::Yaml::str("Comma-wrapped task ids the plan already covered".to_owned()),
            ),
            ("required".to_owned(), yaml::Yaml::Bool(true)),
        ]),
    )];
    let body = composite::composite_yaml_with_inputs(job_id, inputs, rendered)?;
    let bytes = marker::with_marker(&ctx.generator_version, &yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{job_id}/action.yml"),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Job, JobTimeout, StepKind};

    use super::extract_obligation_composites;
    use crate::lane_share::LaneShare;
    use crate::render::RenderContext;
    use crate::steps::shell_step;

    fn context() -> RenderContext {
        RenderContext {
            generator_version: "0.1.0".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            scale_set_selector: None,
            staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
            request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
            checkout_uses: format!("actions/checkout@{:040x}", 0),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            workflow_tasks: Vec::new(),
            pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
            plan_consumer_env: BTreeMap::new(),
        }
    }

    #[test]
    fn oversize_retry_moves_obligation_steps_into_one_composite() {
        let mut obligation = shell_step(
            "Clippy",
            vec!["echo".to_owned(), "crate".to_owned()],
            BTreeMap::new(),
        )
        .expect("step");
        obligation.condition = Some(
            "!contains(needs.plan.outputs.covered_tasks, ',stack/rust/demo/clippy/default,')"
                .to_owned(),
        );
        let job = Job {
            display_name: "Rust / demo".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            check_runner: None,
            steps: vec![obligation],
        };
        let mut shared = LaneShare {
            jobs: BTreeMap::from([("rust-demo".to_owned(), job)]),
            calls: BTreeMap::new(),
            checkouts: BTreeMap::new(),
            env_steps: BTreeMap::new(),
            runtime_preludes: BTreeMap::new(),
            prefixes: BTreeMap::new(),
            preludes: BTreeMap::new(),
            postludes: BTreeMap::new(),
            files: Vec::new(),
        };
        let count = extract_obligation_composites(&mut shared, &context()).expect("extract");
        assert_eq!(count, 1);
        assert_eq!(shared.files[0].path, ".github/actions/rust-demo/action.yml");
        assert!(
            shared.files[0].bytes.contains("inputs.covered_tasks"),
            "composite must read the covered-task input"
        );
        let call = shared.jobs["rust-demo"].steps.first().expect("call");
        assert_eq!(call.name, "Run crate obligations");
        match &call.kind {
            StepKind::Action { uses, with, .. } => {
                assert_eq!(uses, "./.github/actions/rust-demo");
                assert_eq!(
                    with.get("covered_tasks").map(String::as_str),
                    Some("${{needs.plan.outputs.covered_tasks}}")
                );
            }
            other => panic!("expected an action call, got {other:?}"),
        }
    }
}
