// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_JOB_ID;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;
use velnor_actions_contract::{
    CacheAccess, Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, Step, StepKind, StepRole,
    WorkflowIr,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::seed_off::seed_repo;
use super::state_producer_eval::{HEALTHY, LegFaults, legs_of, simulate_leg};
use super::support::rendered_job_block;
use super::waste_oracle::{Files, files_of};

pub(super) const ROOTS: [(&str, &str); 2] = [
    ("state-producer-crates-a-b", STATE_SEAL_JOB_ID),
    ("state-producer-crates-a-c", STATE_SEAL_JOB_ID),
];
const LANE_NAMES: [&str; 4] = [
    "Build test executables",
    "Clippy",
    "Doctests",
    "Documentation",
];

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
        let block = rendered_job_block(text_of(&built, producer_id), producer_id)?;
        assert!(
            block.contains("MBX_BUILD_SCRIPT_EXECUTION: \"0\""),
            "{producer_id}"
        );
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
        let block = rendered_job_block(text_of(&built, id), id)?;
        assert!(
            consumer
                .needs
                .iter()
                .all(|need| !need.starts_with("state-")),
            "{id} needs a state writer"
        );
        if root_of(consumer).is_some() {
            assert!(block.contains("MBX_BUILD_SCRIPT_EXECUTION: \"1\""), "{id}");
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
    for writer in &writers {
        assert!(
            reaches(&built.jobs, writer, STATE_SEAL_JOB_ID),
            "{writer} -> {STATE_SEAL_JOB_ID}"
        );
    }
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
    let seals: Vec<&str> = vec![STATE_SEAL_JOB_ID];
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

    Ok(())
}

#[test]
fn a_failed_producer_lane_or_a_refused_export_uploads_no_payload() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let (producer_id, _) = ROOTS[0];
    let producer = job(&built.jobs, producer_id)?;
    for leg in legs_of(producer) {
        let healthy = simulate_leg(producer, &leg, &HEALTHY)?;
        assert!(healthy.uploaded, "{leg}: a healthy leg uploaded nothing");
        assert!(healthy.lanes >= 4, "{leg}: {} lanes", healthy.lanes);
        for failing in [0, 1, healthy.lanes / 2, healthy.lanes - 1] {
            let failed = LegFaults {
                failing: Some(failing),
                ..HEALTHY
            };
            assert!(
                !simulate_leg(producer, &leg, &failed)?.uploaded,
                "{leg}: lane {failing} failed and the payload was uploaded"
            );
        }
        let refused = LegFaults {
            refuse_export: true,
            ..HEALTHY
        };
        assert!(
            !simulate_leg(producer, &leg, &refused)?.uploaded,
            "{leg}: a refused export was uploaded"
        );
    }
    Ok(())
}

#[test]
fn the_seal_legs_run_one_at_a_time_and_reserve_the_writers_bound() -> Result<(), Box<dyn Error>> {
    let built = build()?;
    let seal = job(&built.jobs, STATE_SEAL_JOB_ID)?;
    assert_eq!(
        seal.matrix.as_ref().map(|matrix| matrix.max_parallel),
        Some(1),
        "the seal legs must run one at a time"
    );
    let block = rendered_job_block(text_of(&built, STATE_SEAL_JOB_ID), STATE_SEAL_JOB_ID)?;
    assert!(
        block.contains("    strategy:\n      fail-fast: false\n      max-parallel: 1\n"),
        "{block}"
    );
    for (producer, _) in ROOTS {
        assert!(
            rendered_job_block(text_of(&built, producer), producer)?
                .contains("    strategy:\n      fail-fast: false\n      max-parallel: 2\n"),
            "{producer}: its legs must not cancel each other"
        );
    }
    let reserve = seal
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::StateSealAdmit))
        .and_then(|step| match &step.kind {
            StepKind::Shell { env, .. } => env.get("VELNOR_STATE_WRITERS_BYTES").cloned(),
            _ => None,
        });
    assert_eq!(
        reserve.as_deref(),
        Some("1744830464"),
        "the admission reserve lost the sources and tools bounds"
    );
    Ok(())
}
