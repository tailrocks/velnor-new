// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUDGET_OUTPUTS, BUDGET_STEP, CacheKey, CargoCommand, GateNeed, HelperCommand, HelperRelease,
    Job, MERGE_GROUP_SKIP_OUTPUT, MERGE_GROUP_STEP, RepoPath, RunnerLabel, Step, StepKind,
    TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT,
    github_hosted, group_output_name,
};

use crate::artifact_steps::{download_step, upload_cargo_log, upload_step};
use crate::base_steps::{
    cache_budget, cache_decide, cache_trim, lane_outputs, sources_fetch, sources_fetch_all,
    sources_locks, trimmed_lane,
};
use crate::cache::{path_text, render_key};
use crate::cargo::{CargoLane, cargo_step, reports_tests};
use crate::gate::{
    PLAN_STEP, Setup, admission, all_conditions, condition, merge_group_skipped, output, plan_read,
    plan_source, sources_step, verdict_step,
};
use crate::helper_steps::restored_step;
use crate::permission_pairs;
use crate::rules::{RenderedJob, RenderedStep, SETUP_VELNOR_ACTION, Value};

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

pub(crate) const TEST_STEP_ID: &str = "test";
pub(crate) const MISE_VERSION: &str = "2026.11.10";
#[path = "steps_helpers.rs"]
mod steps_helpers;
use steps_helpers::{rustup_add, saved_unless_hit, variable};

pub(crate) const CACHE_RESTORE: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
pub(crate) const CACHE_SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const A_NEED_FAILED: &str =
    "${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}";

pub(crate) fn rendered_job(job: &Job, fork_runner: &RunnerLabel) -> RenderedJob {
    let (restores, audit) = crate::base_steps::job_setup(job);
    let lane = crate::cargo::cargo_lane(job);
    let tests = job
        .steps
        .iter()
        .any(|step| matches!(&step.kind, StepKind::Cargo(command) if reports_tests(command)));
    let mut steps =
        crate::helper_integrity::secure_steps(job, job_steps(job, restores, audit, tests, lane));
    let env = crate::cargo::hoist_target_dir(job, &mut steps);
    RenderedJob {
        id: job.id.to_string(),
        name: job.name.clone(),
        comment: job.comment.clone(),
        env,
        needs: job.needs.iter().map(ToString::to_string).collect(),
        condition: Some(crate::base_steps::audited_lane(
            admission(job.when, job.gate.as_ref(), job.pr),
            job,
        )),
        runs_on: job.runs_on.as_str().to_owned(),
        fork_runner: (!github_hosted(job.runs_on.as_str())).then(|| fork_runner.to_string()),
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
            .chain(test_outputs(job))
            .chain(merge_group_output(job))
            .chain(lane_outputs(&job.steps))
            .chain(budget_outputs(job))
            .collect(),
        steps,
        ..RenderedJob::default()
    }
}

fn test_outputs(job: &Job) -> impl Iterator<Item = (String, String)> + '_ {
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
        .map(|name| {
            (
                name.to_owned(),
                format!("${{{{ steps.{TEST_STEP_ID}.outputs.{name} }}}}"),
            )
        })
}

fn merge_group_output(job: &Job) -> Option<(String, String)> {
    job.steps
        .iter()
        .any(|step| step.kind == StepKind::Helper(HelperCommand::MergeGroupPassed))
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
    tests: bool,
    lane: CargoLane,
) -> Vec<RenderedStep> {
    job.steps
        .iter()
        .scan(false, |replanned, step| {
            let setup = Setup::of(restores, *replanned);
            *replanned |= step.kind == StepKind::Helper(HelperCommand::PlanFull);
            Some(crate::base_steps::audited_save(
                job,
                step,
                rendered_steps(step, setup, audit, tests, lane, job.name.as_deref()),
            ))
        })
        .flatten()
        .collect()
}

fn rendered_steps(
    step: &Step,
    setup: Setup,
    audit: bool,
    tests: bool,
    lane: CargoLane,
    job_name: Option<&str>,
) -> Vec<RenderedStep> {
    let mut rendered = Vec::new();
    if audit && step.kind == StepKind::Cargo(CargoCommand::Doctest) {
        rendered.push(cargo_step(&CargoCommand::BuildAll, true, lane));
    }
    rendered.push(step_body(
        &step.kind,
        step.directory.as_ref(),
        audit,
        tests,
        lane,
    ));
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
        extra.extend(
            step.skip_after_merge_group
                .as_ref()
                .map(merge_group_skipped),
        );
        body.condition = all_conditions(
            body.condition.take(),
            condition(step.when, step.gate.as_ref(), setup, step.pr, &extra),
        );
        body.working_directory = step.directory.as_ref().map(ToString::to_string);
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
    rendered
}

fn step_body(
    kind: &StepKind,
    directory: Option<&RepoPath>,
    audit: bool,
    tests: bool,
    lane: CargoLane,
) -> RenderedStep {
    match kind {
        StepKind::ReportCaches { caches, .. } => crate::report::lane(caches, tests),
        StepKind::UploadMetrics => crate::report::artifact(true),
        StepKind::DownloadMetrics => crate::report::artifact(false),
        StepKind::CheckMetrics {
            needs_max_bytes,
            trusted_api,
        } => crate::report::check(*needs_max_bytes, *trusted_api),
        StepKind::Checkout { depth } => RenderedStep {
            name: "Check out".to_owned(),
            uses: Some(CHECKOUT.to_owned()),
            with: vec![
                ("fetch-depth".to_owned(), Value::Int(u64::from(*depth))),
                ("persist-credentials".to_owned(), Value::Bool(false)),
            ],
            ..RenderedStep::default()
        },
        StepKind::InstallTools { only } => install_tools(only.as_deref()),
        StepKind::InstallMise => install_mise(),
        StepKind::Lint(command) => crate::lint_steps::lint_step(command),
        StepKind::UploadArchive { name, file } => upload_step(name, file),
        StepKind::DownloadArchive { name, into } => download_step(name, into.as_ref()),
        StepKind::CacheRestore(key) => cache_step(CACHE_RESTORE, "Restore", key, true),
        StepKind::CacheSave(key) => cache_step(CACHE_SAVE, "Save", key, false),
        StepKind::CacheDecide {
            target,
            archive,
            manifests,
        } => cache_decide(target, archive.as_ref(), manifests),
        StepKind::RestoredTargets { expiry, targets } => restored_step(*expiry, targets),
        StepKind::SourcesLocks(locks) => sources_locks(locks),
        StepKind::CacheResult(lanes) => crate::cache_upload::result(lanes),
        StepKind::CacheVerify { lane, family } => crate::cache_upload::verify(lane, *family),
        StepKind::CacheTrim { lane, family } => cache_trim(lane, *family),
        StepKind::CacheBudget(bytes) => cache_budget(*bytes),
        StepKind::RustComponents(components) => rustup_add("component", components),
        StepKind::RustTargets(targets) => rustup_add("target", targets),
        StepKind::SourcesFetch { position, .. } => sources_fetch(*position, lane),
        StepKind::SourcesFetchAll { manifests, .. } => {
            sources_fetch_all(manifests, lane.cache_debug)
        }
        StepKind::Cargo(command) => cargo_step(command, audit, lane),
        StepKind::Helper(command) => crate::helper_steps::helper_step(*command),
        StepKind::TestGroupsCheck(groups) => crate::helper_steps::test_groups_step(groups),
        StepKind::InputsSave(target) => crate::helper_steps::inputs_step("save", target, directory),
        StepKind::InputsRestore(target) => {
            crate::helper_steps::inputs_step("restore", target, directory)
        }
        StepKind::MbxRemote => crate::helper_steps::mbx_remote_step(),
        StepKind::CratesMirror(mirror) => crate::helper_steps::mirror_step(mirror),
        StepKind::MbxStore => crate::helper_steps::mbx_store_step(),
        StepKind::PrivateCheck { .. }
        | StepKind::PrivateToken { .. }
        | StepKind::PrivateFetch { .. } => crate::private_steps::render(kind),
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

fn budget_outputs(job: &Job) -> Vec<(String, String)> {
    if !job
        .steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Budget { .. }))
    {
        return Vec::new();
    }
    BUDGET_OUTPUTS
        .iter()
        .map(|name| {
            (
                name.to_string(),
                format!("${{{{ steps.{BUDGET_STEP}.outputs.{name} }}}}"),
            )
        })
        .collect()
}

fn budget_step(last: usize) -> RenderedStep {
    RenderedStep {
        name: "Check the speed budget".to_owned(),
        id: Some(BUDGET_STEP.to_owned()),
        env: vec![(
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        )],
        run: Some(format!("velnor-ci bench budget --last {last}")),
        ..RenderedStep::default()
    }
}

fn install_tools(only: Option<&str>) -> RenderedStep {
    let mut with = vec![
        ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
        ("cache".to_owned(), Value::Bool(false)),
    ];
    if let Some(tool) = only {
        with.push(("install_args".to_owned(), Value::Text(tool.to_owned())));
    }
    RenderedStep {
        name: "Install tools".to_owned(),
        uses: Some(MISE.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

fn install_mise() -> RenderedStep {
    RenderedStep {
        name: "Install mise".to_owned(),
        uses: Some(MISE.to_owned()),
        with: vec![
            ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
            ("install".to_owned(), Value::Bool(false)),
            ("cache".to_owned(), Value::Bool(false)),
        ],
        ..RenderedStep::default()
    }
}

fn cache_step(action: &str, verb: &str, key: &CacheKey, fallbacks: bool) -> RenderedStep {
    let rendered = render_key(key);
    let paths: Vec<String> = key.paths().iter().map(path_text).collect();
    let mut with = vec![
        ("path".to_owned(), Value::Text(paths.join("\n"))),
        ("key".to_owned(), Value::Expr(rendered.key)),
    ];
    if fallbacks && !rendered.restore_keys.is_empty() {
        with.push((
            "restore-keys".to_owned(),
            Value::Expr(rendered.restore_keys.join("\n")),
        ));
    }
    RenderedStep {
        id: Some(crate::report::cache_id(key, fallbacks)),
        name: format!("{verb} the {} cache", key.family()),
        uses: Some(action.to_owned()),
        continue_on_error: !fallbacks && crate::cache_upload::key_id(key).is_some(),
        with,
        ..RenderedStep::default()
    }
}

pub(crate) fn install_helper(release: &HelperRelease) -> RenderedStep {
    let version = release.version();
    let mut with = vec![("version".to_owned(), Value::Text(version.to_string()))];
    with.extend(release.digests().iter().map(|(platform, digest)| {
        (
            format!("sha256-{}", platform.file_suffix()),
            Value::Text(digest.to_string()),
        )
    }));
    RenderedStep {
        name: format!("Install velnor-ci {version}"),
        uses: Some(format!("{SETUP_VELNOR_ACTION}@{}", release.action())),
        uses_comment: Some(version.release_tag()),
        with,
        ..RenderedStep::default()
    }
}
