//! Crate-job body extraction for workflows over GitHub's file cap.
//!
//! Inlined obligation commands are unique per crate, so YAML anchors cannot
//! share them. When `ci.yml` would exceed 500 KB, those steps move into one
//! local composite per crate job. The job keeps checkout, tool setup, and
//! report upload. Obligation steps are identified structurally: Rust
//! task executions and the `OpenTofu` provider-cache consumers.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::action_ref::CRATE_OBLIGATION_ACTION_PREFIX;
use crate::lane_share::LaneShare;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, composite, marker, steps, yaml};

/// Move obligation steps out of crate jobs that are still inlined.
///
/// Lane-shared jobs already keep their bodies in a composite. Returns the
/// number of actions written. Zero means this pass cannot shrink the file.
///
/// # Errors
///
/// Returns [`RenderError`] when an obligation block is split or the action
/// path collides.
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
    // Composite actions cannot declare background steps; a job with a
    // parallel group keeps its obligations literal in the workflow.
    if job.steps.iter().any(|step| step.parallel) {
        return Ok(None);
    }
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
    let obligations = job.steps[start..=end].to_vec();
    let file = obligation_action(job_id, &obligations, ctx)?;
    let call = steps::action_step(
        "Run crate obligations",
        &format!("{CRATE_OBLIGATION_ACTION_PREFIX}{job_id}"),
        BTreeMap::new(),
    )?;
    let mut steps = Vec::with_capacity(job.steps.len() - obligations.len() + 1);
    steps.extend(job.steps[..start].iter().cloned());
    steps.push(call);
    steps.extend(job.steps[end + 1..].iter().cloned());
    job.steps = steps;
    Ok(Some(file))
}

fn is_obligation_step(step: &Step) -> bool {
    matches!(step.kind, StepKind::TaskExecution { .. })
        || step.role == Some(StepRole::TofuProviderUse)
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
    let body = composite::composite_yaml(job_id, rendered)?;
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
                    parallel: false,
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
                    parallel: false,
                    name: "Check task".to_owned(),
                    id: None,
                    role: Some(StepRole::TofuProviderUse),
                    condition: None,
                    kind: StepKind::Shell {
                        run: vec!["echo".to_owned(), "task".to_owned()],
                        env: BTreeMap::from([
                            (
                                "TF_PLUGIN_CACHE_DIR".to_owned(),
                                "${{ runner.temp }}/velnor/tofu-cache/alpha".to_owned(),
                            ),
                            ("TF_DATA_DIR".to_owned(), ".terraform".to_owned()),
                        ]),
                    },
                },
            ],
        };
        let ctx = RenderContext {
            generator_version: "0.1.0".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            scale_set_selector: None,
            staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
            request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
            checkout_uses: "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            workflow_tasks: Vec::new(),
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
