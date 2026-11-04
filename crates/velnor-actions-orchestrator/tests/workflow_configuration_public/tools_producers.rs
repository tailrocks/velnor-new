// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::step_identity::{
    TOOLS_CACHE_IDENTITY_DIGEST_INPUT, ToolsRestoreForm,
};
use velnor_actions_contract::workflow::{
    ProducerKind, RUST_VERIFIER_USES, SOURCES_PRODUCER_JOB_ID,
};
use velnor_actions_contract::{
    CacheWriter, Job, REQUIRED_JOB_ID, Step, StepKind, StepRole, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use super::maintenance_features::owner_feature_repo;

const PROBE_MISS: &str = "steps.tools-restore.outputs.cache-hit != 'true'";
pub(super) const POLICIES: [WorkflowPolicy; 2] = [
    WorkflowPolicy::ConsumerV1,
    WorkflowPolicy::VelnorRepositoryV1,
];

pub(super) fn finalized(
    policy: WorkflowPolicy,
) -> Result<(WorkflowIr, BTreeMap<String, Job>), Box<dyn Error>> {
    let repo = owner_feature_repo(policy, false, "default", "default")?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    Ok((prepared.workflow.ir.clone(), jobs))
}

pub(super) fn is_tools_producer(job: &Job) -> bool {
    job.producer_kind() == Some(ProducerKind::ToolsIdentity)
}

pub(super) fn identity_digest(job: &Job) -> Option<&str> {
    job.steps
        .iter()
        .find_map(|step| match (&step.role, &step.kind) {
            (Some(StepRole::ToolsCacheIdentity), StepKind::Action { with, .. }) => with
                .get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
                .map(String::as_str),
            _ => None,
        })
}

pub(super) fn positions(job: &Job, role: StepRole) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(index, _)| index)
        .collect()
}

pub(super) fn is_upload(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@"))
}

fn assert_producer_shape(id: &str, job: &Job) {
    assert_eq!(
        job.condition.as_deref(),
        Some(CacheWriter::ToolsProducer.condition().as_str()),
        "{id}"
    );
    assert!(
        job.needs.is_empty() || job.needs == [SOURCES_PRODUCER_JOB_ID],
        "{id} needs {:?}",
        job.needs
    );
    for step in &job.steps {
        assert!(
            !matches!(step.kind, StepKind::TaskExecution { .. })
                && !step.parallel
                && !is_upload(step),
            "{id} runs {}",
            step.name
        );
    }
    let restores = positions(job, StepRole::ToolsCacheRestore);
    let saves = positions(job, StepRole::ToolsCacheSave);
    assert_eq!((restores.len(), saves.len()), (1, 1), "{id}");
    assert!(restores[0] < saves[0], "{id} saves before its probe");
    assert_eq!(
        ToolsRestoreForm::of(&job.steps[restores[0]].kind),
        Some(ToolsRestoreForm::LookupOnly),
        "{id}"
    );
    let save_condition = job.steps[saves[0]].condition.as_deref().unwrap_or_default();
    assert!(
        save_condition.contains(PROBE_MISS),
        "{id} save is not gated on a probe miss"
    );
    assert_producer_installs(id, job, restores[0], saves[0]);
}

fn install_positions(job: &Job) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(
                step.role,
                Some(
                    StepRole::PreparePinnedTools
                        | StepRole::PrepareRustComponents
                        | StepRole::VerifyRustManifest
                        | StepRole::VerifyJactionlint
                )
            )
        })
        .map(|(index, _)| index)
        .collect()
}

fn assert_producer_installs(id: &str, job: &Job, probe: usize, save: usize) {
    let scrubs = positions(job, StepRole::ToolsCacheScrub);
    assert_eq!(scrubs.len(), 1, "{id}");
    assert!(probe < scrubs[0], "{id} scrubs before its probe");
    let installs = install_positions(job);
    assert!(
        installs
            .iter()
            .any(|at| job.steps[*at].role == Some(StepRole::PreparePinnedTools)),
        "{id} installs nothing"
    );
    for at in installs.iter().chain(&scrubs) {
        let step = &job.steps[*at];
        assert!(
            step.condition
                .as_deref()
                .unwrap_or_default()
                .contains(PROBE_MISS),
            "{id} runs {} on a probe hit",
            step.name
        );
        assert!(
            scrubs[0] <= *at && *at < save,
            "{id} misplaces {}",
            step.name
        );
    }
    assert_eq!(save + 1, job.steps.len(), "{id} has steps after its save");
    for step in &job.steps {
        let verifies_restore =
            matches!(&step.kind, StepKind::Action { uses, .. } if uses == RUST_VERIFIER_USES[0]);
        assert!(!verifies_restore, "{id} verifies a restore it never made");
    }
}

#[test]
fn finalized_jobs_have_one_producer_per_tools_identity() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_, jobs) = finalized(policy)?;
        let producers: Vec<(&String, &Job)> = jobs
            .iter()
            .filter(|(_, job)| is_tools_producer(job))
            .collect();
        let produced: Vec<&str> = producers
            .iter()
            .filter_map(|(_, job)| identity_digest(job))
            .collect();
        let produced_set: BTreeSet<&str> = produced.iter().copied().collect();
        assert_eq!(
            produced.len(),
            produced_set.len(),
            "{policy:?}: duplicate producer identity"
        );
        let consumed: BTreeSet<&str> = jobs
            .values()
            .filter(|job| !is_tools_producer(job))
            .filter_map(identity_digest)
            .collect();
        assert!(
            consumed.len() >= 3,
            "{policy:?}: fixture yields {} identities",
            consumed.len()
        );
        assert_eq!(produced_set, consumed, "{policy:?}");
        for (id, job) in producers {
            assert_producer_shape(id, job);
        }
    }
    Ok(())
}

#[test]
fn required_plan_and_crate_jobs_never_need_a_producer() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_, jobs) = finalized(policy)?;
        let producer_ids: BTreeSet<&str> = jobs
            .iter()
            .filter(|(_, job)| is_tools_producer(job))
            .map(|(id, _)| id.as_str())
            .collect();
        assert!(
            !producer_ids.is_empty(),
            "{policy:?}: no producers to check against"
        );
        assert!(jobs.contains_key(REQUIRED_JOB_ID));
        for (id, job) in &jobs {
            let leaked: Vec<&String> = job
                .needs
                .iter()
                .filter(|need| producer_ids.contains(need.as_str()))
                .collect();
            assert!(
                leaked.is_empty(),
                "{policy:?}: {id} needs producers {leaked:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn staged_tree_renders_producers_for_both_policies() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let prepared = prepare(repo.path())?;
        let tree = render_staged_tree(&prepared)?;
        let workflow = tree
            .get(velnor_actions_workflow_renderer::render::WORKFLOW_PATH)
            .ok_or("rendered workflow is missing")?;
        assert!(
            workflow.contains("  tools-producer-"),
            "{policy:?}: no producer job rendered"
        );
    }
    Ok(())
}
