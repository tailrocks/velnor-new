// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    Access, CacheDebug, CacheKey, CacheRoot, CargoCommand, Concurrency, FullRun, HelperCommand,
    Host, Job, JobId, MbxStore, PackageName, PermissionScope, Permissions, PullRequestType,
    RepoFacts,
    RootSlug, RustRoot, Shape, ShapeDecision, Step, StepKind, TargetName, Toolchain, Triggers,
    When, WorkflowPlan,
};

use crate::audit_job::audit_job;
use crate::base_cache::restored;
use crate::config::CONFIG_PATH;
use crate::detect::HELPER_PACKAGE;
use crate::error::{DetectError, Reason};
use crate::lane::{Lane, PLAN_JOB, plan_job, policy_job, used_outputs};
use crate::required_job::{expose_in_single, gate_outputs, has_gate};
use crate::root_jobs::root_jobs;
use crate::schedule::schedule;
use crate::shape::decide_shape;

pub(crate) const RUST_JOB: JobId = JobId::from_static("rust");
pub(crate) const LINT_JOB: JobId = JobId::from_static("rust-lint");
pub(crate) const TEST_JOB: JobId = JobId::from_static("rust-test");
pub(crate) const DEFAULT_GROUP_JOB: JobId = JobId::from_static("rust-test-default");
pub(crate) const HELPER_JOB: JobId = JobId::from_static("rust-helper");
pub(crate) const BUDGET_JOB: JobId = JobId::from_static(velnor_model::BUDGET_JOB);
pub(crate) const CHECKOUT_DEPTH: u32 = 1;
pub(crate) const PLAN_DEPTH: u32 = 2;
const TIE_BREAKING_HELPER: (u64, u64, u64) = (0, 1, 81);
const BUDGET_RUNS: usize = 10;
const BUDGET_TIMEOUT_MINUTES: u16 = 10;

#[must_use]
pub fn plan(facts: &RepoFacts) -> WorkflowPlan {
    let suffixed = facts.rust_roots.len() > 1;
    let decisions: Vec<ShapeDecision> = facts
        .rust_roots
        .iter()
        .map(|root| decide_shape(facts.config.shape, facts.timings.get(&root.manifest)))
        .collect();
    let helper_root = facts.config.helper.as_ref().and_then(|path| {
        facts
            .rust_roots
            .iter()
            .find(|root| root.manifest.parent().as_ref() == Some(path))
    });
    let single =
        facts.rust_roots.len() == 1 && decisions[0].shape == Shape::OneJob && helper_root.is_none();
    let mut jobs: Vec<Job> = facts
        .rust_roots
        .iter()
        .zip(decisions)
        .flat_map(|(root, decision)| {
            let lane = Lane::root(RootSlug::of(&root.manifest), single);
            root_jobs(facts, root, decision, suffixed, &lane)
        })
        .collect();
    jobs.extend(audit_job(facts, &jobs));
    if facts.policy.any() {
        jobs.push(policy_job(facts, &Lane::policy(single)));
    }
    let draft_guard = facts
        .config
        .skip_draft_pull_requests
        .then(|| step(StepKind::FailOnDraftRun));
    let gate = has_gate(facts);
    if !single {
        let mut used = used_outputs(&jobs);
        if gate {
            used.extend(gate_outputs(&jobs));
            used.sort();
            used.dedup();
        }
        jobs.insert(0, plan_job(facts, used));
    } else if gate {
        expose_in_single(&mut jobs);
    }
    if let Some(root) = helper_root {
        jobs.insert(1, helper_job(facts, root));
    }
    if facts.config.cache_debug == CacheDebug::On {
        upload_cargo_logs(&mut jobs);
    }
    if facts.config.budget.is_some_and(|budget| budget.enabled()) {
        jobs.push(budget_job(facts));
    }
    crate::report::finish(facts, &mut jobs, single, draft_guard);
    WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: PullRequestType::ALL
                .into_iter()
                .chain(
                    facts
                        .config
                        .skip_draft_pull_requests
                        .then_some(PullRequestType::ConvertedToDraft),
                )
                .collect(),
            push_branch: Some(facts.config.default_branch.clone()),
            merge_group: true,
            schedule: schedule(facts),
            workflow_dispatch: true,
        },
        concurrency: Concurrency::CancelOlderPullRequestRuns,
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        fork_runner: facts.config.fork_runner.clone(),
        jobs,
    }
}

fn budget_job(facts: &RepoFacts) -> Job {
    let mut steps = vec![step(StepKind::Checkout {
        depth: CHECKOUT_DEPTH,
    })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    steps.push(step(StepKind::Budget { last: BUDGET_RUNS }));
    Job {
        name: Some("Speed budget".to_owned()),
        when: if facts.config.full_run == FullRun::Off {
            When::DefaultBranchPush
        } else {
            When::Scheduled
        },
        permissions: Permissions(BTreeMap::from([
            (PermissionScope::Actions, Access::Read),
            (PermissionScope::Contents, Access::Read),
        ])),
        timeout_minutes: BUDGET_TIMEOUT_MINUTES,
        steps,
        ..job(facts, BUDGET_JOB)
    }
}

fn helper_job(facts: &RepoFacts, root: &RustRoot) -> Job {
    let lane = Lane {
        slug: RootSlug::of(&root.manifest),
        plan: Some(PLAN_JOB),
    };
    let mut steps = setup(facts, root, &lane, &HELPER_JOB);
    steps.push(Step {
        directory: root.manifest.parent(),
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

pub(crate) fn head(facts: &RepoFacts, depth: u32) -> Vec<Step> {
    let mut steps = vec![step(StepKind::Checkout { depth })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    if facts.config.mbx_store == MbxStore::Shared {
        steps.push(step(StepKind::MbxStore));
    }
    steps
}

pub(crate) fn mirror_step(facts: &RepoFacts) -> Option<Step> {
    facts
        .config
        .crates_mirror
        .as_ref()
        .filter(|mirror| mirror.runner == facts.config.runner)
        .map(|mirror| step(StepKind::CratesMirror(mirror.clone())))
}

pub(crate) fn tools_cache_key() -> CacheKey {
    CacheKey::Tools { host: Host }
}

pub(crate) fn tools_restore(facts: &RepoFacts) -> Option<Step> {
    (!facts.tools.tools.is_empty()).then(|| step(StepKind::CacheRestore(tools_cache_key())))
}

pub(crate) fn tools_save(facts: &RepoFacts) -> Option<Step> {
    (!facts.tools.tools.is_empty()).then(|| Step {
        when: When::SaveToolsOnDefaultBranch,
        ..step(StepKind::CacheSave(tools_cache_key()))
    })
}

pub(crate) fn tools_install(facts: &RepoFacts) -> Option<Step> {
    (!facts.tools.tools.is_empty()).then(|| step(StepKind::InstallTools { only: None }))
}

pub(crate) fn setup(facts: &RepoFacts, root: &RustRoot, lane: &Lane, job: &JobId) -> Vec<Step> {
    setup_roots(
        facts,
        std::slice::from_ref(root),
        lane,
        std::slice::from_ref(job),
    )
}

fn rust_additions(declared: Option<&Toolchain>, root: &RustRoot) -> Vec<Step> {
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

pub(crate) fn setup_roots(
    facts: &RepoFacts,
    roots: &[RustRoot],
    lane: &Lane,
    families: &[JobId],
) -> Vec<Step> {
    let root = &roots[0];
    let mut steps = head(
        facts,
        if lane.plan.is_none() {
            PLAN_DEPTH
        } else {
            CHECKOUT_DEPTH
        },
    );
    if lane.plan.is_none() {
        steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    }
    let mut toolchain: Vec<Step> = [
        mirror_step(facts),
        tools_restore(facts),
        tools_install(facts),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !facts.tools.tools.is_empty() {
        toolchain.extend(mirror_step(facts));
    }
    let declared = facts.toolchain.as_ref();
    toolchain.extend(rust_additions(declared, root));
    let prepare = step(StepKind::Helper(HelperCommand::Prepare));
    if declared.is_some_and(|toolchain| toolchain.profile.as_deref() == Some("complete")) {
        toolchain.push(prepare);
        toolchain.extend(tools_save(facts));
    } else {
        toolchain.extend(tools_save(facts));
        toolchain.push(prepare);
    }
    for (root, family) in roots.iter().zip(families) {
        let cache_root = CacheRoot::new(&root.manifest, root.target_dir.clone());
        toolchain.extend([
            Step {
                directory: root.manifest.parent(),
                ..step(StepKind::Helper(HelperCommand::CheckMetadata))
            },
            step(StepKind::CacheRestore(CacheKey::Sources {
                host: Host,
                root: cache_root.clone(),
            })),
            step(StepKind::CacheRestore(CacheKey::Target {
                job: family.clone(),
                root: cache_root,
                host: Host,
            })),
        ]);
    }
    if breaks_ties(facts) {
        toolchain.extend(restore_file_times(roots));
    }
    toolchain.extend(restored(facts));
    steps.extend(toolchain.into_iter().map(|step| Step {
        gate: lane.steps(),
        ..step
    }));
    steps
}

fn upload_cargo_logs(jobs: &mut [Job]) {
    for job in jobs {
        if job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Cargo(_)))
        {
            let gate = job
                .steps
                .iter()
                .find(|step| {
                    matches!(
                        step.kind,
                        StepKind::Helper(HelperCommand::Restored(..))
                            | StepKind::RestoredTargets { .. }
                    )
                })
                .and_then(|step| step.gate.clone());
            job.steps.push(Step {
                when: When::CacheDebug,
                gate,
                ..step(StepKind::UploadCargoLog)
            });
        }
    }
}

fn breaks_ties(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(TIE_BREAKING_HELPER))
}

fn restore_file_times(roots: &[RustRoot]) -> impl Iterator<Item = Step> {
    roots.iter().map(|root| Step {
        directory: root.manifest.parent(),
        ..step(StepKind::InputsRestore(root.target_dir.clone()))
    })
}

pub(crate) fn job(facts: &RepoFacts, id: JobId) -> Job {
    Job {
        id,
        name: None,
        comment: None,
        needs: Vec::new(),
        when: When::Success,
        skip_on_draft: facts.config.skip_draft_pull_requests,
        runs_on: facts.config.runner.clone(),
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        timeout_minutes: facts.config.timeout_minutes,
        gate: None,
        plan_outputs: Vec::new(),
        steps: Vec::new(),
    }
}

pub(crate) fn step(kind: StepKind) -> Step {
    Step {
        kind,
        directory: None,
        when: When::Success,
        skip_on_draft: false,
        gate: None,
        reads: None,
    }
}
