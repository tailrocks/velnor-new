// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CargoCommand, GateNeed, HelperCommand, Job, MERGE_GROUP_SKIP_OUTPUT,
    MERGE_GROUP_STEP, Step, StepKind, TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT,
    TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT, WorkflowPlan, group_output_name,
};

use crate::artifact_steps::{download_step, upload_cargo_log, upload_step};
use crate::base_steps::{
    cache_budget, cache_decide, cache_trim, lane_outputs, sources_fetch, sources_fetch_all,
    sources_fetch_all_rest, sources_locks, trimmed_lane,
};
use crate::budget_steps::budget_step;
use crate::cache::cache_step;
use crate::cargo::{CargoLane, cargo_step, reports_tests};
use crate::checkout_step::checkout_step;
use crate::gate::{
    PLAN_STEP, Setup, admission, all_conditions, condition, merge_group_skipped, output, plan_read,
    plan_source, sources_step, verdict_step,
};
use crate::helper_steps::{inputs_step, restore_inputs_step, restored_targets_step};
pub(crate) use crate::install_steps::install_helper;
use crate::install_steps::{install_mise, install_tools};
use crate::permission_pairs;
use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::save_gate::SaveGate;
use crate::tests::Tests;

pub(crate) const PREPARE_STEP: &str = "prepare";
pub(crate) const RUN: &str = "velnor-ci run --";
pub(crate) const RUN_DOCTESTS: &str = "velnor-ci run --require-doctests=$DOCTESTS_KNOWN --";
pub(crate) const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
pub(crate) const MISE: &str = "jdx/mise-action@236a21c3b91f5db6d4e174b348674a0f0caac06d";
pub(crate) const UPLOAD_ARTIFACT: &str =
    "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";
pub(crate) const DOWNLOAD_ARTIFACT: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";

pub(crate) const ATTEST_BUILD_PROVENANCE: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";

pub(crate) const MISE_VERSION: &str = "2026.11.10";
#[path = "steps_helpers.rs"]
mod steps_helpers;
use steps_helpers::{rustup_add, saved_unless_hit, variable};

pub(crate) const CACHE_RESTORE: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
pub(crate) const CACHE_SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const A_NEED_FAILED: &str =
    "${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}";

pub(crate) fn rendered_job(job: &Job, plan: &WorkflowPlan) -> RenderedJob {
    let (restores, audit) = crate::base_steps::job_setup(job);
    let lane = crate::cargo::cargo_lane(job);
    let tests = Tests::of(job);
    let mut steps =
        crate::helper_integrity::secure_steps(job, job_steps(job, restores, audit, &tests, lane));
    let env = crate::cargo::hoist_target_dir(job, &mut steps);
    RenderedJob {
        id: job.id.to_string(),
        name: job.name.clone(),
        check_name: crate::selection_name::title(job),
        comment: job.comment.clone(),
        env,
        needs: job.needs.iter().map(ToString::to_string).collect(),
        condition: Some(crate::base_steps::audited_lane(
            admission(
                job.when,
                job.gate.as_ref(),
                job.pr,
                &job.needs,
                job.skip_after_merge_group.as_ref(),
            ),
            job,
        )),
        runs_on: job.runs_on.as_str().to_owned(),
        fork_runner: (!plan.hosted_runners.contains(&job.runs_on))
            .then(|| plan.fork_runner.to_string()),
        permissions: permission_pairs(&job.permissions),
        timeout_minutes: job.timeout_minutes,
        outputs: job
            .plan_outputs
            .iter()
            .map(|(slug, need)| {
                let key = output(slug, *need);
                let value = format!("${{{{ steps.{PLAN_STEP}.outputs.{key} }}}}");
                (key, value)
            })
            .chain(job.group_outputs.iter().map(|(slug, name)| {
                let key = group_output_name(slug, name);
                let value = format!("${{{{ steps.{PLAN_STEP}.outputs.{key} }}}}");
                (key, value)
            }))
            .chain(test_outputs(job, &tests))
            .chain(merge_group_output(job))
            .chain(lane_outputs(&job.steps))
            .collect(),
        cache_mode: crate::cache::saving_mode(job),
        steps,
        ..RenderedJob::default()
    }
}

fn test_outputs<'a>(job: &'a Job, tests: &'a Tests) -> impl Iterator<Item = (String, String)> + 'a {
    job.steps
        .iter()
        .any(|step| matches!(&step.kind, StepKind::Cargo(command) if reports_tests(command)))
        .then(|| {
            [
                Some(TESTS_RUN_OUTPUT),
                Some(TESTS_LISTED_OUTPUT),
                job.reports_ignored.then_some(TESTS_IGNORED_OUTPUT),
                Some(TESTS_BY_PACKAGE_OUTPUT),
            ]
        })
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|name| tests.outputs(name))
}

fn merge_group_output(job: &Job) -> Option<(String, String)> {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::MergeGroupPassed { .. }))
        .then(|| {
            (
                MERGE_GROUP_SKIP_OUTPUT.to_owned(),
                format!("${{{{ steps.{MERGE_GROUP_STEP}.outputs.skip }}}}"),
            )
        })
}

fn job_steps(
    job: &Job,
    restores: bool,
    audit: bool,
    tests: &Tests,
    lane: CargoLane,
) -> Vec<RenderedStep> {
    let gate = SaveGate::of(job);
    let verified = job
        .steps
        .iter()
        .any(|step| step.kind == StepKind::ToolsLocal(velnor_model::ToolsLocalVerb::Verify));
    job.steps
        .iter()
        .enumerate()
        .scan(false, |replanned, (index, step)| {
            let setup = Setup::of(restores, *replanned);
            *replanned |= step.kind == StepKind::Helper(HelperCommand::PlanFull);
            let around = Around {
                id: gate.id(index),
                saved_after: gate.required(),
                job_name: job.name.as_deref(),
            };
            let mut rendered = rendered_steps(step, setup, audit, tests, lane, around);
            let tools_key = matches!(
                &step.kind,
                StepKind::CacheRestore(velnor_model::CacheKey::Tools { .. })
                    | StepKind::CacheSave(velnor_model::CacheKey::Tools { .. })
            );
            for body in &mut rendered {
                body.shared |= verified && tools_key;
            }
            Some(crate::base_steps::audited_save(
                job,
                step,
                rendered,
                gate.audit_runs(index),
            ))
        })
        .flatten()
        .collect()
}

/// What the job around a step adds to it.
#[derive(Clone, Copy)]
struct Around<'a> {
    /// The id a compile or audit step gets so a save can wait for it.
    id: Option<&'a str>,
    saved_after: &'a [String],
    job_name: Option<&'a str>,
}

fn rendered_steps(
    step: &Step,
    setup: Setup,
    audit: bool,
    tests: &Tests,
    lane: CargoLane,
    around: Around<'_>,
) -> Vec<RenderedStep> {
    let job_name = around.job_name;
    let mut rendered = Vec::new();
    if audit && step.kind == StepKind::Cargo(CargoCommand::Doctest) {
        rendered.push(cargo_step(
            &CargoCommand::BuildAll,
            step.manifest.as_ref(),
            true,
            lane,
            None,
        ));
    }
    let mut body = step_body(step, audit, tests, lane);
    if body.id.is_none() {
        body.id = around.id.map(str::to_owned);
    }
    rendered.push(body);
    if let StepKind::SourcesFetchAll { manifests, .. } = &step.kind {
        rendered.extend(sources_fetch_all_rest(
            manifests,
            lane.cache_debug,
            lane.offline,
        ));
    }
    for body in &mut rendered {
        if let (StepKind::Cargo(_), Some(target)) = (&step.kind, &step.target_dir) {
            body.env.push(crate::cargo::target_dir_env(target));
        }
        if matches!(step.kind, StepKind::ReportCaches { .. })
            && let Some(name) = job_name
        {
            body.env.push((
                "VELNOR_REPORT_JOB_NAME".to_owned(),
                Value::Text(name.to_owned()),
            ));
        }
        let mut extra = saved_unless_hit(&step.kind);
        extra.extend(crate::private_steps::repository_conditions(step));
        extra.extend(
            step.hosted_only
                .then(|| crate::gate::NOT_SELF_HOSTED.to_owned()),
        );
        extra.extend(
            step.skip_when_cold
                .then(|| crate::gate::PREPARE_COLD.to_owned()),
        );
        extra.extend(
            step.skip_after_merge_group
                .as_ref()
                .map(|from| merge_group_skipped(from, setup)),
        );
        body.condition = all_conditions(
            body.condition.take(),
            condition(
                step.when,
                step.gate.as_ref(),
                setup,
                step.pr,
                &extra,
                around.saved_after,
            ),
        );
        body.env.extend(crate::private_steps::decide_env(step));
        body.working_directory = step.directory.as_ref().map(ToString::to_string);
        body.shared |= matches!(step.when, velnor_model::When::SaveVerifiedTools(_));
        if matches!(step.kind, StepKind::InputsSave { .. })
            && step.when != velnor_model::When::SaveBase
        {
            body.id = None;
        }
    }
    if let (Some(plan), StepKind::Cargo(command), Some(main)) =
        (&step.reads, &step.kind, rendered.last_mut())
    {
        let source = plan_source(&plan.from);
        main.env.extend(command.plan_values().iter().map(|need| {
            let key = output(&plan.slug, *need);
            let read = plan_read(&source, &key, setup);
            (
                variable(*need).to_owned(),
                Value::Expr(format!("${{{{ {read} }}}}")),
            )
        }));
    }
    if let Some(verify) = rendered
        .first()
        .and_then(|saved| crate::cache_upload::record(&step.kind, saved))
    {
        rendered.push(verify);
    }
    if let Some(note) = rendered.first().and_then(|saved| {
        crate::cache_upload::after_test_failure(&step.kind, saved, tests.failed(), |extra| {
            condition(
                velnor_model::When::NotCancelled,
                step.gate.as_ref(),
                setup,
                step.pr,
                extra,
                &[],
            )
        })
    }) {
        rendered.push(note);
    }
    rendered
}

fn step_body(step: &Step, audit: bool, tests: &Tests, lane: CargoLane) -> RenderedStep {
    let kind = &step.kind;
    let directory = step.directory.as_ref();
    match kind {
        StepKind::ReportCaches {
            caches, node_tools, ..
        } => crate::report::lane(caches, *node_tools, tests.first_id()),
        StepKind::UploadMetrics => crate::report::artifact(true),
        StepKind::DownloadMetrics => crate::report::artifact(false),
        StepKind::CheckMetrics {
            needs_max_bytes,
            trusted_api,
        } => crate::report::check(*needs_max_bytes, *trusted_api),
        StepKind::Checkout { depth, sparse } => checkout_step(*depth, sparse.as_deref()),
        StepKind::InstallTools { only } => install_tools(only.as_deref()),
        StepKind::InstallMise => install_mise(),
        StepKind::Lint(command) => crate::lint_steps::lint_step(command),
        StepKind::UploadArchive { name, file } => upload_step(name, file),
        StepKind::DownloadArchive { name, into } => download_step(name, into.as_ref()),
        StepKind::CacheRestore(key) => cache_step(CACHE_RESTORE, "Restore", key, true),
        StepKind::CacheLookup(key) => crate::tools_steps::tools_lookup(key),
        StepKind::CacheSave(key) => cache_step(CACHE_SAVE, "Save", key, false),
        StepKind::CacheDecide {
            target,
            archive,
            manifests,
            cli,
            budget_bytes,
        } => cache_decide(target, archive.as_ref(), manifests, *cli, *budget_bytes),
        StepKind::SourcesLocks(locks) => sources_locks(locks),
        StepKind::CacheResult(lanes) => crate::cache_upload::result(lanes),
        StepKind::CacheVerify { lane, family } => crate::cache_upload::verify(lane, *family),
        StepKind::CacheTrim { lane, family } => cache_trim(lane, *family),
        StepKind::CacheBudget(bytes) => cache_budget(*bytes),
        StepKind::DeltaPrepare(root) => crate::delta_steps::delta_step(root, "prepare"),
        StepKind::DeltaApply(root) => crate::delta_steps::delta_step(root, "apply"),
        StepKind::DeltaStage(root, packages) => crate::delta_steps::delta_stage(root, packages),
        StepKind::RustComponents(components) => rustup_add("component", components),
        StepKind::RustTargets(targets) => rustup_add("target", targets),
        StepKind::SourcesFetch { position, .. } => sources_fetch(*position, lane),
        StepKind::SourcesFetchAll { manifests, .. } => {
            sources_fetch_all(manifests, lane.cache_debug, lane.offline)
        }
        StepKind::Cargo(command) => cargo_step(
            command,
            step.manifest.as_ref(),
            audit,
            lane,
            reports_tests(command).then(|| tests.id(step.reads.as_ref().map(|plan| &plan.slug))),
        ),
        StepKind::Helper(command) => crate::helper_steps::helper_step(*command),
        StepKind::Prepare(locks) => crate::helper_steps::prepare_step(locks),
        StepKind::InputsAuditRoot(directory) => {
            crate::helper_steps::audit_root_step(directory.as_ref())
        }
        StepKind::MergeGroupPassed { required } => crate::helper_steps::merge_group_step(required),
        StepKind::MergeQueueCheck => crate::helper_steps::merge_queue_step(),
        StepKind::TestGroupsCheck(groups) => crate::helper_steps::test_groups_step(groups),
        StepKind::InputsSave { target, cli } => inputs_step("save", target, directory, *cli),
        StepKind::InputsRestore { roots, cli } => restore_inputs_step(roots, directory, *cli),
        StepKind::RestoredTargets {
            targets,
            expiry,
            cli,
        } => restored_targets_step(targets, *expiry, *cli),
        StepKind::MbxRemote { from_pin } => crate::helper_steps::mbx_remote_step(*from_pin),
        StepKind::MbxPin => crate::helper_steps::mbx_pin_step(),
        StepKind::CratesMirror(mirror) => crate::helper_steps::mirror_step(mirror),
        StepKind::MbxStore | StepKind::MbxPrivate => crate::helper_steps::mbx_local_step(kind),
        StepKind::RepositoryPrivacy
        | StepKind::PrivateCheck { .. }
        | StepKind::PrivateToken { .. }
        | StepKind::PrivateFetch { .. }
        | StepKind::ReleaseAssets { .. } => crate::private_steps::render(kind),
        StepKind::AdvisoryFetch { configs } => crate::private_steps::advisory_fetch(configs),
        StepKind::MbxSeed | StepKind::ToolsLocal(_) => crate::helper_steps::node_step(kind),
        StepKind::TestSetup { name, run } => RenderedStep {
            name: name.clone(),
            run: Some(run.clone()),
            ..RenderedStep::default()
        },
        StepKind::GeneratorCheck => crate::helper_steps::generator_check_step(),
        StepKind::InstallHelper(release) => install_helper(release),
        StepKind::FailOnDraftRun => crate::draft_steps::fail_on_draft(),
        StepKind::DraftNotice => crate::draft_steps::draft_notice(),
        StepKind::ClearRunLabel => crate::draft_steps::clear_run_label(),
        StepKind::UploadCargoLog => upload_cargo_log(),
        StepKind::Verdict {
            spec,
            needs_max_bytes,
        } => verdict_step(spec, *needs_max_bytes),
        StepKind::FailWhenANeedFailed => RenderedStep {
            name: "Fail when a Rust job failed or was cancelled".to_owned(),
            condition: Some(A_NEED_FAILED.to_owned()),
            run: Some("exit 1".to_owned()),
            ..RenderedStep::default()
        },
        StepKind::Budget { last } => budget_step(*last),
    }
}
