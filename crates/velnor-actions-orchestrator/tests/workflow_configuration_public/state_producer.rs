// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;
use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCER_LANE_CONDITION, STATE_SEAL_PENDING_BYTES_ENV, STATE_SEAL_PENDING_ENV,
};
use velnor_actions_contract::{
    CacheAccess, Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, Step, StepKind, StepRole,
    WorkflowIr,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::mbx_job_env::assert_build_script_execution;
use super::seed_off::seed_repo;
use super::state_producer_eval::{seal_runs, simulate};
use super::support::rendered_job_block;
use super::waste_oracle::{Files, files_of};

pub(super) const ROOTS: [(&str, &str); 2] = [
    ("state-producer-crates-a-b", "state-seal-crates-a-b"),
    ("state-producer-crates-a-c", "state-seal-crates-a-c"),
];
const LANE_NAMES: [&str; 4] = [
    "Build test executables",
    "Clippy",
    "Doctests",
    "Documentation",
];
const UNIT_BOUND: &str = "1616920640";

pub(super) struct Built {
    pub(super) ir: WorkflowIr,
    pub(super) jobs: BTreeMap<String, Job>,
    pub(super) ci: String,
    pub(super) seal: String,
    pub(super) files: Files,
}

pub(super) fn text_of<'a>(built: &'a Built, id: &str) -> &'a str {
    if built.seal.contains(&format!("\n  {id}:\n")) {
        &built.seal
    } else {
        &built.ci
    }
}

pub(super) fn build() -> Result<Built, Box<dyn Error>> {
    let repo = seed_repo("")?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let tree = render_staged_tree(&prepared)?;
    let files = files_of(&tree)?;
    let ci = files
        .get(WORKFLOW_PATH)
        .ok_or("ci.yml was not rendered")?
        .clone();
    let seal = files
        .get(STATE_SEAL_WORKFLOW_PATH)
        .ok_or("state-seal.yml was not rendered")?
        .clone();
    Ok(Built {
        ir: prepared.workflow.ir.clone(),
        jobs,
        ci,
        seal,
        files,
    })
}

pub(super) fn job<'a>(
    jobs: &'a BTreeMap<String, Job>,
    id: &str,
) -> Result<&'a Job, Box<dyn Error>> {
    jobs.get(id)
        .ok_or_else(|| format!("{id} was not generated").into())
}

fn root_of(job: &Job) -> Option<&str> {
    job.steps
        .iter()
        .find_map(|step| match (&step.role, &step.kind) {
            (Some(StepRole::MbxBundleKey), StepKind::Shell { env, .. }) => {
                env.get("VELNOR_MBX_BUNDLE_ROOT").map(String::as_str)
            }
            _ => None,
        })
}

fn crate_jobs_of<'a>(jobs: &'a BTreeMap<String, Job>, root: &str) -> Vec<&'a Job> {
    jobs.iter()
        .filter(|(id, job)| id.starts_with("rust-") && root_of(job) == Some(root))
        .map(|(_, job)| job)
        .collect()
}

fn lanes(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| {
            matches!(step.kind, StepKind::TaskExecution { .. })
                && LANE_NAMES.contains(&step.name.as_str())
        })
        .collect()
}

fn effective(job: &Job, ir: &WorkflowIr) -> Permissions {
    job.permissions
        .clone()
        .unwrap_or_else(|| ir.permissions.clone())
}

fn reaches(jobs: &BTreeMap<String, Job>, from: &str, to: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack = vec![to.to_owned()];
    while let Some(id) = stack.pop() {
        if id == from {
            return true;
        }
        if seen.insert(id.clone())
            && let Some(job) = jobs.get(&id)
        {
            stack.extend(job.needs.iter().cloned());
        }
    }
    false
}

#[test]
fn producer_runs_the_crate_job_lane_records() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    for ((producer_id, _), root) in ROOTS.iter().zip(["crates/a-b", "crates/a/c"]) {
        let producer = job(&built.jobs, producer_id)?;
        assert_eq!(producer.producer_kind(), Some(ProducerKind::StateProducer));
        assert_eq!(producer.cache_access, CacheAccess::Read);
        assert_eq!(producer.needs, ["plan"]);
        assert_eq!(root_of(producer), Some(root));
        let wanted: Vec<&Step> = crate_jobs_of(&built.jobs, root)
            .into_iter()
            .flat_map(lanes)
            .collect();
        assert!(wanted.iter().any(|s| s.name == "Doctests"), "{root}");
        let repeated = lanes(producer);
        assert_eq!(repeated.len(), 2 * wanted.len(), "{root}: two phases");
        for lane in &wanted {
            let copies = repeated
                .iter()
                .filter(|step| step.kind == lane.kind && step.name == lane.name)
                .count();
            assert_eq!(copies, 2, "{root}: {} runs once per phase", lane.name);
        }
        let ids: BTreeSet<String> = wanted
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::TaskExecution { task_id, .. } => Some(task_id.clone()),
                _ => None,
            })
            .collect();
        for step in &producer.steps {
            if let StepKind::TaskExecution { task_id, .. } = &step.kind {
                assert!(ids.contains(task_id), "{root}: foreign task {task_id}");
                assert_eq!(
                    step.condition.as_deref(),
                    Some(STATE_PRODUCER_LANE_CONDITION)
                );
            }
        }
        let uploads = producer
            .steps
            .iter()
            .filter(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@")))
            .count();
        assert_eq!(uploads, 1, "{root}: only the handoff artifact is uploaded");
        assert!(
            producer
                .steps
                .iter()
                .all(|step| !step.name.contains("ledger") && !step.name.contains("reports")),
            "{root}: the producer has no ledger or report step"
        );
        assert_build_script_execution(text_of(&built, producer_id), producer_id, "0")?;
        for consumer in crate_jobs_of(&built.jobs, root) {
            assert!(
                consumer.steps.iter().all(|step| !matches!(
                    step.role,
                    Some(StepRole::MbxBundleExport | StepRole::StateSealSave)
                )),
                "{root}: a crate job exports or saves state"
            );
        }
    }
    for (id, consumer) in built.jobs.iter().filter(|(id, _)| id.starts_with("rust-")) {
        assert!(
            consumer
                .needs
                .iter()
                .all(|need| !need.starts_with("state-")),
            "{id} needs a state writer"
        );
        if root_of(consumer).is_some() {
            assert_build_script_execution(text_of(&built, id), id, "1")?;
        }
    }
    Ok(())
}

#[test]
fn writers_are_chained_by_needs_without_concurrency() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let writers: Vec<&String> = built
        .jobs
        .keys()
        .filter(|id| id.as_str() == "sources-producer" || id.starts_with("tools-producer-"))
        .collect();
    assert!(writers.len() >= 2, "expected sources and tools writers");
    let [(_, first), (_, second)] = ROOTS;
    for writer in &writers {
        assert!(reaches(&built.jobs, writer, first), "{writer} -> {first}");
    }
    assert!(reaches(&built.jobs, first, second), "{first} -> {second}");
    assert!(
        !reaches(&built.jobs, second, first),
        "the chain must be one-way"
    );
    for (producer, seal) in ROOTS {
        assert!(
            job(&built.jobs, seal)?
                .needs
                .iter()
                .any(|need| need == producer)
        );
    }
    let tools: Vec<&String> = writers
        .iter()
        .copied()
        .filter(|id| id.starts_with("tools-producer-"))
        .collect();
    for id in tools {
        assert!(
            reaches(&built.jobs, "sources-producer", id),
            "sources -> {id}"
        );
    }
    for id in writers
        .iter()
        .map(|id| id.as_str())
        .chain(ROOTS.iter().flat_map(|(p, s)| [*p, *s]))
    {
        assert!(
            !rendered_job_block(text_of(&built, id), id)?.contains("concurrency:"),
            "{id} carries a concurrency group"
        );
    }
    let required = job(&built.jobs, REQUIRED_JOB_ID)?;
    for (producer, seal) in ROOTS {
        assert!(
            !required
                .needs
                .iter()
                .any(|need| need == producer || need == seal)
        );
    }
    let gate = rendered_job_block(text_of(&built, "required-gate"), "required-gate")?;
    assert!(!gate.contains("state-producer") && !gate.contains("state-seal"));
    Ok(())
}

#[test]
fn only_attest_jobs_hold_actions_write() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let seals: Vec<&str> = ROOTS.iter().map(|(_, seal)| *seal).collect();
    for (id, candidate) in &built.jobs {
        let permissions = effective(candidate, &built.ir);
        let writes = permissions.actions == PermissionLevel::Write;
        assert_eq!(writes, seals.contains(&id.as_str()), "{id}");
    }
    for seal in &seals {
        let block = rendered_job_block(text_of(&built, seal), seal)?;
        let permissions = effective(job(&built.jobs, seal)?, &built.ir);
        assert_eq!(permissions.id_token, PermissionLevel::Write);
        assert_eq!(permissions.attest, PermissionLevel::Write);
        assert_eq!(permissions.contents, PermissionLevel::Read);
        assert_eq!(permissions.pull_requests, PermissionLevel::None);
        assert_eq!(permissions.packages, PermissionLevel::None);
        assert!(
            !block.contains("actions/checkout"),
            "{seal} checks out code"
        );
        let job_env = block
            .split("    steps:")
            .next()
            .ok_or("job header missing")?;
        assert!(!job_env.contains("GH_TOKEN"), "{seal}: job-level GH_TOKEN");
    }
    for (producer, _) in ROOTS {
        let permissions = effective(job(&built.jobs, producer)?, &built.ir);
        assert_eq!(permissions.id_token, PermissionLevel::None);
        assert_eq!(permissions.attest, PermissionLevel::None);
        assert_ne!(permissions.actions, PermissionLevel::Write);
        assert_ne!(permissions.contents, PermissionLevel::Write);
    }
    let mut crafted = built.jobs.clone();
    let victim = crafted.get_mut(ROOTS[0].0).ok_or("producer missing")?;
    victim.permissions = Some(Permissions {
        actions: PermissionLevel::Write,
        ..effective(victim, &built.ir)
    });
    let mut candidate = built.ir.clone();
    candidate.jobs = crafted;
    assert!(
        candidate.validate().is_err(),
        "a producer with actions: write validated"
    );

    let [(_, first), (_, second)] = ROOTS;
    let first_job = job(&built.jobs, first)?;
    let second_job = job(&built.jobs, second)?;
    let pending = |seal: &Job| -> Vec<(String, String)> {
        seal.steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::Shell { env, .. } if env.contains_key(STATE_SEAL_PENDING_ENV) => Some((
                    env[STATE_SEAL_PENDING_ENV].clone(),
                    env[STATE_SEAL_PENDING_BYTES_ENV].clone(),
                )),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        pending(first_job),
        [(
            "${{needs.plan.outputs.state_union_crates_a_c_produce}}:velnor-v4-state-trusted-union-crates-a-c-"
                .to_owned(),
            UNIT_BOUND.to_owned()
        )]
    );
    assert_eq!(
        pending(second_job),
        [(String::new(), UNIT_BOUND.to_owned())]
    );
    Ok(())
}

#[test]
fn a_failed_producer_lane_leaves_no_produced_output_and_the_seal_does_not_run()
-> Result<(), Box<dyn Error>> {
    let built = build()?;
    let (producer_id, seal_id) = ROOTS[0];
    let producer = job(&built.jobs, producer_id)?;
    let seal = job(&built.jobs, seal_id)?;
    assert!(
        seal.condition
            .as_deref()
            .is_some_and(|condition| !condition.contains(".result")),
        "the seal gate reads a result, which is success for a failed continue-on-error job"
    );
    let flag = |outputs: &BTreeMap<String, String>| outputs.get("produced").cloned();

    let healthy = simulate(producer, None, false)?;
    assert_eq!(flag(&healthy).as_deref(), Some("true"));
    assert!(
        seal_runs(seal, producer_id, &healthy)?,
        "a healthy producer must seal"
    );

    let lane_steps = producer
        .steps
        .iter()
        .filter(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .count();
    assert!(lane_steps >= 4);
    for failing in [0, 1, lane_steps / 2, lane_steps - 1] {
        let outputs = simulate(producer, Some(failing), false)?;
        assert_eq!(
            flag(&outputs).as_deref(),
            Some(""),
            "lane {failing} failed but produced was set"
        );
        assert!(
            !seal_runs(seal, producer_id, &outputs)?,
            "lane {failing} failed and the seal ran"
        );
    }
    let refused = simulate(producer, None, true)?;
    assert!(
        !seal_runs(seal, producer_id, &refused)?,
        "a refused export was sealed"
    );
    Ok(())
}
