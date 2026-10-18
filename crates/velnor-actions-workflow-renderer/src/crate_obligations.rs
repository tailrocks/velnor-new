//! Crate-job body extraction for workflows over GitHub's file cap.
//!
//! Inlined obligation commands are unique per crate, so YAML anchors cannot
//! share them. When `ci.yml` would exceed 500 KB, those steps move into one
//! local composite per crate job. The job keeps checkout, tool setup, and
//! report upload, and passes covered-task ids in, because composite steps
//! cannot read `needs`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step};

use crate::action_ref::CRATE_OBLIGATION_ACTION_PREFIX;
use crate::lane_share::LaneShare;
use crate::render::{COVERED_TASKS_OUTPUT, PLAN_JOB_ID, RenderContext};
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
        &format!("{CRATE_OBLIGATION_ACTION_PREFIX}{job_id}"),
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
                yaml::Yaml::str(
                    "Task IDs already in the plan, with one comma before and after each ID."
                        .to_owned(),
                ),
            ),
            ("required".to_owned(), yaml::Yaml::Bool(true)),
        ]),
    )];
    let body = composite::composite_yaml_with_inputs(job_id, inputs, rendered)?;
    let bytes = marker::with_marker(&ctx.generator_version, &yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/crate-obligation-{job_id}/action.yml"),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

    #[test]
    fn extracts_job_starting_with_provider_prelude() {
        let job_id = "tofu-provider-prelude-alpha";
        let mut job = Job {
            display_name: "tofu provider prelude alpha".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            cache_mode: None,
            environment: None,
            check_runner: None,
            steps: vec![
                Step {
                    name: "Checkout".to_owned(),
                    id: None,
                    role: None,
                    condition: None,
                    kind: StepKind::Action {
                        uses: "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683"
                            .to_owned(),
                        with: BTreeMap::new(),
                        env: BTreeMap::new(),
                    },
                },
                Step {
                    name: "Check task".to_owned(),
                    id: None,
                    role: None,
                    condition: Some(format!(
                        "contains(needs.{PLAN_JOB_ID}.outputs.{COVERED_TASKS_OUTPUT}, 'tofu-task')"
                    )),
                    kind: StepKind::Shell {
                        run: vec!["echo".to_owned(), "task".to_owned()],
                        env: BTreeMap::new(),
                    },
                },
            ],
        };
        let ctx = RenderContext {
            generator_version: "0.1.0".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            scale_set_selector: None,
            scale_set_image_source_sha: None,
            verified_cache_helpers: std::collections::BTreeMap::new(),
            require_verified_cache_helper: false,
            staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
            request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
            checkout_uses: "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            workflow_tasks: Vec::new(),
            pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
            mise_quiet: true,
            plan_consumer_env: BTreeMap::new(),
        };
        let file = extract_job(job_id, &mut job, &ctx)
            .expect("extraction must succeed")
            .expect("obligation file written");
        assert_eq!(
            file.path,
            ".github/actions/crate-obligation-tofu-provider-prelude-alpha/action.yml"
        );
        let call_step = &job.steps[1];
        let StepKind::Action { uses, .. } = &call_step.kind else {
            panic!("expected Action step");
        };
        assert_eq!(
            uses,
            "./.github/actions/crate-obligation-tofu-provider-prelude-alpha"
        );
        crate::action_ref::validate_uses(uses).expect("action step uses must validate");
    }
}
