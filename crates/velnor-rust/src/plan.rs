// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    Access, CacheDebug, CacheKey, CargoCommand, Concurrency, Gate, HelperCommand, Host, Job, JobId,
    MbxRemote, MbxStore, PackageName, PermissionScope, Permissions, PrCi, PrRun, PrepareLocks,
    PushAfterMergeGroup, RepoFacts, RootSlug, RustRoot, Step, StepKind, TargetName, Toolchain,
    When, WorkflowPlan,
};

use crate::audit_job::audit_job;
use crate::base_cache::{restored, sources_locks};
use crate::config::CONFIG_PATH;
use crate::detect::HELPER_PACKAGE;
use crate::error::{DetectError, Reason};
use crate::lane::{
    Lane, PLAN_JOB, expose_read_outputs, plan_job, policy_job, used_group_outputs, used_outputs,
};
use crate::mbx_pin::MBX_PIN_HELPER;
use crate::plan_budget::{draft_guard, expose_selection, triggers};
use crate::plan_tools::{mirror_step, tools_install, tools_restore, tools_save};
use crate::required_job::{expose_in_single, gate_outputs};
use crate::root_jobs::{lane_jobs, root_jobs};
use crate::shape::{decisions, helper_root, single_job};
use crate::units::{Unit, units};

pub(crate) const RUST_JOB: JobId = JobId::from_static("rust");
pub(crate) const LINT_JOB: JobId = JobId::from_static("rust-lint");
pub(crate) const TEST_JOB: JobId = JobId::from_static("rust-test");
pub(crate) const MBX_SEED_HELPER: (u64, u64, u64) = (0, 1, 86);
pub(crate) const TOOLS_LOCAL_HELPER: (u64, u64, u64) = (0, 1, 89);
pub(crate) const DEFAULT_GROUP_JOB: JobId = JobId::from_static("rust-test-default");
pub(crate) const HELPER_JOB: JobId = JobId::from_static("rust-helper");
pub(crate) const BUDGET_JOB: JobId = JobId::from_static(velnor_model::BUDGET_JOB);
pub(crate) const CHECKOUT_DEPTH: u32 = 1;
pub(crate) const PLAN_DEPTH: u32 = 2;
const IGNORED_COUNT_HELPER: (u64, u64, u64) = (0, 1, 86);

#[must_use]
pub fn plan(facts: &RepoFacts) -> WorkflowPlan {
    plan_with(facts, true)
}

#[must_use]
pub fn plan_without_deltas(facts: &RepoFacts) -> WorkflowPlan {
    plan_with(facts, false)
}

fn plan_with(facts: &RepoFacts, deltas: bool) -> WorkflowPlan {
    let units = units(facts);
    let suffixed = units.len() > 1;
    let decisions = decisions(facts, &units);
    let helper_root = helper_root(facts);
    let single = single_job(facts, &units, &decisions);
    let mut jobs: Vec<Job> = units
        .iter()
        .zip(decisions.iter().cloned())
        .flat_map(|(unit, decision)| {
            let lane = Lane::root(unit.gate.clone(), single, deltas);
            root_jobs(facts, unit, decision, suffixed, &lane)
        })
        .collect();
    jobs.extend(audit_job(facts, &units, &jobs));
    for (unit, decision) in units.iter().zip(&decisions) {
        let lane = Lane::root(unit.gate.clone(), single, deltas);
        jobs.extend(lane_jobs(facts, unit, decision.shape, suffixed, &lane));
    }
    if facts.policy.any() {
        jobs.push(policy_job(facts, &Lane::policy(single, deltas)));
    }
    jobs.extend(crate::lint_jobs::lint_jobs(facts));
    let draft_guard = draft_guard(facts);
    if single {
        expose_in_single(&mut jobs);
        expose_read_outputs(&mut jobs, &RUST_JOB);
    } else {
        let mut used = used_outputs(&jobs);
        used.extend(gate_outputs(&jobs));
        used.sort();
        used.dedup();
        jobs.insert(0, plan_job(facts, used, used_group_outputs(&jobs)));
    }
    expose_selection(facts, &mut jobs, single, &units);
    if let Some(root) = helper_root {
        jobs.insert(1, helper_job(facts, root, deltas));
    }
    apply_extensions(facts, &mut jobs, single);
    crate::report::finish(facts, &mut jobs, single, draft_guard);
    WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: triggers(facts),
        concurrency: match facts.config.pr_ci {
            PrCi::Always => Concurrency::CancelOlderPullRequestRuns,
            PrCi::Label => Concurrency::CancelOlderPullRequestRunsExceptUnrelatedLabels,
        },
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        fork_runner: facts.config.fork_runner.clone(),
        hosted_runners: facts.config.hosted_runners.clone(),
        jobs,
    }
}

fn apply_extensions(facts: &RepoFacts, jobs: &mut [Job], single: bool) {
    if facts.config.cache_debug == CacheDebug::On {
        crate::cargo_log::upload(jobs);
    }
    crate::private_fetch::insert(facts.config.private_sources.as_ref(), jobs);
    crate::lock_scope::apply(facts, jobs);
    if facts.config.mbx_remote == MbxRemote::GithubActions && facts.private_sources.is_none() {
        add_mbx_remote(facts, jobs);
    }
    if facts.config.push_after_merge_group == PushAfterMergeGroup::SkipWhenQueueRunPassed {
        crate::merge_group_skip::apply(jobs, single);
    }
    crate::docs_post_merge::apply(facts, jobs);
}

fn helper_job(facts: &RepoFacts, root: &RustRoot, deltas: bool) -> Job {
    let lane = Lane {
        slug: RootSlug::of(&root.manifest),
        plan: Some(PLAN_JOB),
        per_root: false,
        deltas,
    };
    let unit = Unit::single(root);
    let mut steps = setup_units(
        facts,
        std::slice::from_ref(&unit),
        &lane,
        std::slice::from_ref(&HELPER_JOB),
    );
    steps.push(Step {
        directory: root.manifest.parent(),
        target_dir: Some(root.target_dir.clone()),
        ..step(StepKind::Cargo(CargoCommand::BuildBinary {
            package: PackageName::from_static(HELPER_PACKAGE),
            binary: TargetName::from_static(HELPER_PACKAGE),
        }))
    });
    Job {
        needs: vec![PLAN_JOB],
        gate: lane.job(),
        steps,
        ..job(facts, HELPER_JOB)
    }
}
pub fn check_required_check(facts: &RepoFacts, plan: &WorkflowPlan) -> Result<(), DetectError> {
    let mut ids = BTreeSet::new();
    if let Some(job) = plan.jobs.iter().find(|job| !ids.insert(job.id.as_str())) {
        return Err(DetectError::JobIdTaken(job.id.to_string()));
    }
    let name = &facts.config.required_check;
    if plan.jobs.iter().any(|job| {
        job.id.as_str() != "required" && job.name.as_deref().unwrap_or(job.id.as_str()) == name
    }) {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::InvalidValue {
                key: "required_check",
                value: name.clone(),
            },
        ));
    }
    Ok(())
}

pub(crate) fn head(facts: &RepoFacts, depth: u32, sparse: Option<Vec<String>>) -> Vec<Step> {
    let mut steps = vec![step(StepKind::Checkout { depth, sparse })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    steps
}

/// The shared-store steps. A job that plans runs them after the plan, so a
/// run that selects nothing never seeds: `seed_gate` is the job's selection.
pub(crate) fn mbx(facts: &RepoFacts, seed_gate: Option<Gate>) -> Vec<Step> {
    if facts.config.mbx_store != MbxStore::Shared {
        return Vec::new();
    }
    let seed = facts
        .helper
        .as_ref()
        .is_some_and(|release| release.version().at_least(MBX_SEED_HELPER))
        .then(|| Step {
            gate: seed_gate,
            ..step(StepKind::MbxSeed)
        });
    [Some(step(StepKind::MbxStore)), seed]
        .into_iter()
        .flatten()
        .collect()
}

pub(crate) fn rust_additions(declared: Option<&Toolchain>, root: &RustRoot) -> Vec<Step> {
    let mut steps = Vec::new();
    if declared.is_none_or(|toolchain| toolchain.path.is_none()) {
        let mut components: Vec<String> = declared
            .map(|toolchain| toolchain.components.clone())
            .unwrap_or_default();
        if declared.is_some_and(|toolchain| toolchain.profile.as_deref() == Some("default")) {
            components.push("rust-docs".to_owned());
        }
        components.extend(["rustfmt".to_owned(), "clippy".to_owned()]);
        components.sort();
        components.dedup();
        let targets = declared
            .map(|toolchain| toolchain.targets.clone())
            .unwrap_or_default();
        for (kind, names) in [
            (StepKind::RustComponents(components.clone()), &components),
            (StepKind::RustTargets(targets.clone()), &targets),
        ] {
            if !names.is_empty() {
                steps.push(Step {
                    directory: root.manifest.parent(),
                    ..step(kind)
                });
            }
        }
    }
    steps
}

pub(crate) fn setup_units(
    facts: &RepoFacts,
    units: &[Unit],
    lane: &Lane,
    families: &[JobId],
) -> Vec<Step> {
    let root = &units[0].roots[0];
    let roots: Vec<RustRoot> = units.iter().flat_map(|unit| unit.roots.clone()).collect();
    let mut steps = head(
        facts,
        if lane.plan.is_none() {
            PLAN_DEPTH
        } else {
            CHECKOUT_DEPTH
        },
        lane.plan
            .as_ref()
            .and_then(|_| crate::sparse::paths(facts, &roots)),
    );
    if lane.plan.is_none() {
        steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    }
    steps.extend(mbx(facts, lane.steps()));
    let mut toolchain: Vec<Step> = mirror_step(facts)
        .into_iter()
        .chain(tools_restore(facts))
        .chain(tools_install(facts))
        .collect();
    if !facts.tools.tools.is_empty() {
        toolchain.extend(mirror_step(facts));
    }
    let declared = facts.toolchain.as_ref();
    toolchain.extend(rust_additions(declared, root));
    let prepare = step(StepKind::Prepare(PrepareLocks::All));
    if declared.is_some_and(|toolchain| toolchain.profile.as_deref() == Some("complete")) {
        toolchain.push(prepare);
        toolchain.extend(tools_save(facts));
    } else {
        toolchain.extend(tools_save(facts));
        toolchain.push(prepare);
    }
    for (index, (unit, family)) in units.iter().zip(families).enumerate() {
        for member in &unit.roots {
            toolchain.push(Step {
                directory: member.manifest.parent(),
                ..step(StepKind::Helper(HelperCommand::CheckMetadata))
            });
        }
        let cache_root = unit.cache_root(facts);
        if index == 0 {
            toolchain.push(sources_locks(facts));
            toolchain.push(step(StepKind::CacheRestore(CacheKey::Sources {
                host: Host,
                lane: cache_root.clone(),
            })));
        }
        toolchain.push(step(StepKind::CacheRestore(CacheKey::Target {
            job: family.clone(),
            root: cache_root,
            host: Host,
        })));
        toolchain.extend(crate::delta_cache::restore(facts, unit, family, lane));
    }
    toolchain.extend(crate::units::restore_file_times(facts, units));
    toolchain.extend(restored(facts));
    steps.extend(toolchain.into_iter().map(|step| Step {
        gate: lane.steps(),
        ..step
    }));
    steps
}

fn add_mbx_remote(facts: &RepoFacts, jobs: &mut [Job]) {
    let from_pin = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(MBX_PIN_HELPER));
    for job in jobs {
        if !job
            .steps
            .iter()
            .any(|step| matches!(&step.kind, StepKind::Cargo(command) if command.builds()))
        {
            continue;
        }
        if let Some(at) = job
            .steps
            .iter()
            .position(|step| matches!(step.kind, StepKind::InstallTools { .. }))
        {
            let installed = &job.steps[at];
            let (gate, pr) = (installed.gate.clone(), installed.pr);
            let mut inserted = Vec::new();
            if from_pin {
                inserted.push(Step {
                    gate: gate.clone(),
                    pr,
                    ..step(StepKind::MbxPin)
                });
            }
            inserted.push(Step {
                gate,
                pr,
                ..step(StepKind::MbxRemote { from_pin })
            });
            let tail = job.steps.split_off(at + 1);
            job.steps.extend(inserted);
            job.steps.extend(tail);
        }
    }
}

pub(crate) fn quick(facts: &RepoFacts) -> PrRun {
    match facts.config.pr_ci {
        PrCi::Label => PrRun::Quick,
        PrCi::Always => PrRun::Always,
    }
}

pub(crate) fn full(facts: &RepoFacts) -> PrRun {
    match facts.config.pr_ci {
        PrCi::Label => PrRun::Requested,
        PrCi::Always if facts.config.skip_draft_pull_requests => PrRun::Ready,
        PrCi::Always => PrRun::Always,
    }
}

pub(crate) fn job(facts: &RepoFacts, id: JobId) -> Job {
    Job {
        id,
        name: None,
        comment: None,
        needs: Vec::new(),
        when: When::Success,
        pr: full(facts),
        runs_on: facts.config.runner.clone(),
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        timeout_minutes: facts.config.timeout_minutes,
        gate: None,
        skip_after_merge_group: None,
        plan_outputs: Vec::new(),
        group_outputs: Vec::new(),
        reports_ignored: facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(IGNORED_COUNT_HELPER)),
        records_build: facts.helper.as_ref().is_some_and(|helper| {
            helper
                .version()
                .at_least(crate::root_jobs::BUILT_RECORD_HELPER)
        }),
        selection_name: None,
        steps: Vec::new(),
    }
}

pub(crate) fn step(kind: StepKind) -> Step {
    Step {
        kind,
        directory: None,
        manifest: None,
        target_dir: None,
        when: When::Success,
        skip_after_merge_group: None,
        skip_when_cold: false,
        pr: PrRun::Always,
        gate: None,
        reads: None,
    }
}
