//! One composite action per duplicated verification lane.
//!
//! GitHub will not start a workflow file larger than 500 KB. Inlining both
//! lane step lists crosses that limit, so the steps live in one action and
//! each lane job keeps its own id, `runs-on`, and `needs`.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, Step};

use crate::composite::composite_yaml;
use crate::document::step_to_yaml;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, marker, steps, yaml::render_yaml};

/// CI workflow plus composite actions for duplicated lanes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedWorkflow {
    /// Marked `ci.yml` bytes.
    pub yaml: String,
    /// One composite action per shared logical job. Empty when no lane pair exists.
    pub shared: Vec<RenderedFile>,
}

/// Jobs rewritten to call a shared action, plus those action files.
#[derive(Debug)]
pub(crate) struct LaneShare {
    /// Same jobs, with shared lanes keeping their headers and empty steps.
    pub jobs: BTreeMap<String, Job>,
    /// Job id to local `uses` path.
    pub calls: BTreeMap<String, String>,
    /// Composite action files, one per logical job.
    pub files: Vec<RenderedFile>,
}

/// Factor `__hosted` / `__local` pairs whose step lists match.
///
/// # Errors
///
/// A pair whose timeout, condition, permissions, environment, or steps
/// differ fails closed. Elected cache saves (`Save Mise tools`, `Save Tofu
/// providers`) stay on the job that owns them and are not part of that
/// comparison. An unsafe logical id fails closed.
pub(crate) fn share_lanes(
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<LaneShare, RenderError> {
    let mut calls = BTreeMap::new();
    let mut files = Vec::new();
    let mut next = jobs.clone();
    for hosted_id in hosted_ids(jobs) {
        let Some(logical) = logical_id(&hosted_id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_lane_id:{hosted_id}"
            )));
        };
        let local_id = format!("{logical}{SCALE_SUFFIX}");
        let Some(local) = jobs.get(&local_id) else {
            continue;
        };
        let Some(hosted) = jobs.get(&hosted_id) else {
            continue;
        };
        let Some((common, hosted_extra, local_extra)) = split_pair(hosted, local) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "lane_body_differs:{logical}"
            )));
        };
        let uses = format!("$/.github/actions/{logical}");
        files.push(composite_file(logical, &common, ctx)?);
        calls.insert(hosted_id.clone(), uses.clone());
        calls.insert(local_id.clone(), uses);
        set_steps(&mut next, &hosted_id, hosted_extra);
        set_steps(&mut next, &local_id, local_extra);
    }
    Ok(LaneShare {
        jobs: next,
        calls,
        files,
    })
}

fn hosted_ids(jobs: &BTreeMap<String, Job>) -> Vec<String> {
    jobs.keys()
        .filter(|id| id.ends_with(HOSTED_SUFFIX))
        .cloned()
        .collect()
}

fn logical_id(hosted_id: &str) -> Option<&str> {
    let logical = hosted_id.strip_suffix(HOSTED_SUFFIX)?;
    let ok = !logical.is_empty()
        && logical
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    ok.then_some(logical)
}

fn split_pair(hosted: &Job, local: &Job) -> Option<(Vec<Step>, Vec<Step>, Vec<Step>)> {
    if hosted.timeout_minutes != local.timeout_minutes
        || hosted.condition != local.condition
        || hosted.permissions != local.permissions
        || hosted.environment != local.environment
    {
        return None;
    }
    let (hosted_common, hosted_extra) = peel_saves(&hosted.steps);
    let (local_common, local_extra) = peel_saves(&local.steps);
    (hosted_common == local_common).then_some((hosted_common, hosted_extra, local_extra))
}

fn peel_saves(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut extra = Vec::new();
    for step in steps {
        if is_elected_save(step) {
            extra.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, extra)
}

fn is_elected_save(step: &Step) -> bool {
    step.name == crate::cache_steps::TOOLS_SAVE_NAME
        || step.name == crate::tofu_cache::TOFU_PROVIDERS_SAVE_NAME
}

fn set_steps(jobs: &mut BTreeMap<String, Job>, id: &str, steps: Vec<Step>) {
    if let Some(job) = jobs.get_mut(id) {
        job.steps = steps;
    }
}

fn composite_file(
    logical: &str,
    steps: &[Step],
    ctx: &RenderContext,
) -> Result<RenderedFile, RenderError> {
    let mut rendered = Vec::with_capacity(steps.len());
    for step in steps {
        rendered.push(step_to_yaml(logical, step, ctx, &[], true)?);
    }
    let body = composite_yaml(logical, rendered)?;
    let quoted = crate::yaml::quote_run_values_in_yaml(body);
    let bytes = marker::with_marker(&ctx.generator_version, &render_yaml(&quoted))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{logical}/action.yml"),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{
        Concurrency, Job, JobTimeout, Permissions, SCALE_SET_NAME, ScaleSetSelector, Step,
        StepKind, Trigger, VELNOR_LABEL, WorkflowIr,
    };

    use super::{HOSTED_SUFFIX, SCALE_SUFFIX, share_lanes};
    use crate::RenderError;
    use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

    const HOSTED_RUNS: &str = "ubuntu-26.04";
    const CAP: usize = 500_000;
    const LOGICAL_JOBS: usize = 21;
    const STEPS_PER_JOB: usize = 48;

    fn ctx() -> RenderContext {
        RenderContext {
            generator_version: "0.1.0".to_owned(),
            runs_on: HOSTED_RUNS.to_owned(),
            staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
            request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
            checkout_uses: format!("actions/checkout@{:040x}", 0),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            plan_consumer_env: BTreeMap::new(),
        }
    }

    fn workflow_ir() -> WorkflowIr {
        WorkflowIr {
            name: "CI".to_owned(),
            triggers: Trigger {
                pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                push_branches: vec!["main".to_owned()],
                merge_group: true,
                workflow_dispatch: None,
                schedule: None,
            },
            permissions: Permissions::default(),
            concurrency: Concurrency {
                group: CONCURRENCY_GROUP.to_owned(),
                cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
            },
            jobs: BTreeMap::new(),
        }
    }

    fn scale_token() -> String {
        ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )
        .expect("scale selector")
        .token()
    }

    fn echo_step(index: usize, payload: &str) -> Step {
        Step {
            name: format!("echo {index}"),
            condition: None,
            kind: StepKind::Shell {
                run: vec!["echo".to_owned(), payload.to_owned()],
                env: BTreeMap::new(),
            },
        }
    }

    fn lane_job(display: &str, runs_on: &str, steps: Vec<Step>) -> Job {
        Job {
            display_name: display.to_owned(),
            runs_on: runs_on.to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps,
        }
    }

    fn render_jobs(
        ir: &WorkflowIr,
        jobs: &BTreeMap<String, Job>,
        ctx: &RenderContext,
        calls: &BTreeMap<String, String>,
    ) -> Result<String, crate::RenderError> {
        let document = crate::document::workflow_to_yaml(ir, jobs, ctx, calls)?;
        let quoted = crate::yaml::quote_run_values_in_yaml(document);
        crate::marker::with_marker(&ctx.generator_version, &crate::yaml::render_yaml(&quoted))
    }

    fn heavy_steps(payload: &str) -> Vec<Step> {
        (0..STEPS_PER_JOB)
            .map(|index| echo_step(index, payload))
            .collect()
    }

    fn paired(steps: &[Step]) -> BTreeMap<String, Job> {
        let scale = scale_token();
        let mut jobs = BTreeMap::new();
        for index in 0..LOGICAL_JOBS {
            let logical = format!("rust-{index}");
            jobs.insert(
                format!("{logical}{HOSTED_SUFFIX}"),
                lane_job(&format!("{logical} hosted"), HOSTED_RUNS, steps.to_vec()),
            );
            jobs.insert(
                format!("{logical}{SCALE_SUFFIX}"),
                lane_job(&format!("{logical} local"), &scale, steps.to_vec()),
            );
        }
        jobs
    }

    #[test]
    fn shared_lanes_keep_ci_under_github_file_cap() {
        let payload = "a".repeat(400);
        let jobs = paired(&heavy_steps(&payload));
        let ir = workflow_ir();
        let context = ctx();
        let unshared = render_jobs(&ir, &jobs, &context, &BTreeMap::new()).expect("unshared");
        assert!(
            unshared.len() > CAP,
            "unshared render must exceed the GitHub cap, got {}",
            unshared.len()
        );
        let shared = share_lanes(&jobs, &context).expect("share");
        let yaml = render_jobs(&ir, &shared.jobs, &context, &shared.calls).expect("shared");
        assert!(
            yaml.len() <= CAP,
            "shared ci.yml must fit, got {}",
            yaml.len()
        );
        assert!(
            !yaml.contains(&payload),
            "shared ci.yml still inlines steps"
        );
        assert!(yaml.contains("runs-on: ubuntu-26.04"));
        assert!(yaml.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
        assert!(yaml.contains("uses: $/.github/actions/rust-0"));
        assert_eq!(shared.files.len(), LOGICAL_JOBS);
        for file in &shared.files {
            assert!(
                file.bytes.len() <= CAP,
                "{} is {} bytes",
                file.path,
                file.bytes.len()
            );
            assert!(file.bytes.contains("shell: bash"), "{}", file.path);
            assert!(file.bytes.contains(&payload), "{}", file.path);
            assert!(file.path.ends_with("/action.yml"), "{}", file.path);
        }
    }

    #[test]
    fn differing_lane_bodies_fail_closed() {
        let step = echo_step(0, "one");
        let mut jobs = paired(&[step]);
        jobs.get_mut("rust-0__hosted").expect("hosted").steps.pop();
        let err = share_lanes(&jobs, &ctx()).expect_err("differs");
        assert!(
            matches!(err, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
            "{err}"
        );
    }

    #[test]
    fn unsafe_logical_id_fails_closed() {
        let step = echo_step(0, "one");
        let mut jobs = BTreeMap::new();
        jobs.insert(
            "rust.0__hosted".to_owned(),
            lane_job("hosted", HOSTED_RUNS, vec![step.clone()]),
        );
        jobs.insert(
            "rust.0__local".to_owned(),
            lane_job("local", &scale_token(), vec![step]),
        );
        let err = share_lanes(&jobs, &ctx()).expect_err("bad id");
        assert!(
            matches!(err, RenderError::InvalidWorkflow(ref problem) if problem == "bad_lane_id:rust.0__hosted"),
            "{err}"
        );
    }

    #[test]
    fn unpaired_jobs_stay_inline() {
        let mut jobs = BTreeMap::new();
        jobs.insert(
            "actionlint".to_owned(),
            lane_job("actionlint", HOSTED_RUNS, vec![echo_step(0, "one")]),
        );
        let shared = share_lanes(&jobs, &ctx()).expect("share");
        assert!(shared.calls.is_empty());
        assert!(shared.files.is_empty());
        let kept = shared.jobs.get("actionlint").expect("actionlint");
        assert_eq!(kept.steps.len(), 1);
    }

    #[test]
    fn elected_save_stays_on_the_winner_job() {
        let mut jobs = paired(&[echo_step(0, "one")]);
        let save = crate::cache_steps::tools_save_step("mise-v1").expect("save");
        jobs.get_mut("rust-0__hosted")
            .expect("hosted")
            .steps
            .push(save);
        let shared = share_lanes(&jobs, &ctx()).expect("share");
        let hosted = shared.jobs.get("rust-0__hosted").expect("hosted");
        let local = shared.jobs.get("rust-0__local").expect("local");
        assert_eq!(hosted.steps.len(), 1);
        assert_eq!(hosted.steps.first().expect("save").name, "Save Mise tools");
        assert!(local.steps.is_empty());
        let action = shared
            .files
            .iter()
            .find(|file| file.path == ".github/actions/rust-0/action.yml")
            .expect("composite");
        assert!(!action.bytes.contains("Save Mise tools"));
        let yaml = render_jobs(&workflow_ir(), &shared.jobs, &ctx(), &shared.calls).expect("yaml");
        assert_eq!(yaml.matches("Save Mise tools").count(), 1);
    }
}
