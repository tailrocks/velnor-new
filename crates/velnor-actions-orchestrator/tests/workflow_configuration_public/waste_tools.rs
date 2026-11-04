// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
use velnor_actions_contract::workflow::{ProducerKind, SOURCES_PRODUCER_JOB_ID};
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, Job, PLAN_JOB_ID, StepKind, StepRole, TOFU_JOB_ID_PREFIX, WorkflowPolicy,
};
use velnor_actions_orchestrator::{
    WasteFinding, WasteRule, finalized_jobs, prepare, waste_findings,
};

use super::one_job_per_crate::mbx_repo_for;
use super::tools_producers::{identity_digest, is_tools_producer};
use super::workflow_configuration_fixture;

const LANE_TOOLS: [&str; 7] = [
    "rust",
    "mr-boxington",
    "jactionlint",
    "shellcheck",
    "nextest",
    "opentofu",
    "bun",
];

const OTHER_LANE_USES: &str = "./.github/actions/u24";
const OTHER_LANE_RUNS_ON: &str = "ubuntu-24.04";

fn lane_jobs() -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    let repo = mbx_repo_for(true, WorkflowPolicy::VelnorRepositoryV1)?;
    let root = repo.path();
    for (dir, name) in [
        ("crates/cli", "velnor-actions-cli"),
        ("crates/orchestrator", "velnor-actions-orchestrator"),
        ("crates/mise", "velnor-actions-mise"),
    ] {
        workflow_configuration_fixture::add_independent_mbx_package(root, dir, name)?;
        workflow_configuration_fixture::write_nextest_task(&root.join(dir))?;
    }
    fs::write(
        root.join("mise.toml"),
        include_str!("../../../../mise.toml"),
    )?;
    fs::write(
        root.join("mise.lock"),
        include_str!("../../../../mise.lock"),
    )?;
    Ok(finalized_jobs(&prepare(root)?)?)
}

fn is_lane_job(id: &str, job: &Job) -> bool {
    id != PLAN_JOB_ID
        && matches!(
            job.producer_kind(),
            None | Some(ProducerKind::StateProducer)
        )
        && job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::PrepareRustComponents))
}

fn install_run(job: &Job) -> Result<String, Box<dyn Error>> {
    job.steps
        .iter()
        .find_map(|step| match (&step.role, &step.kind) {
            (Some(StepRole::PreparePinnedTools), StepKind::Shell { run, .. }) => {
                Some(run.join(" "))
            }
            _ => None,
        })
        .ok_or_else(|| "job has no pinned tools install".into())
}

fn installs(run: &str, tool: &str) -> bool {
    run.contains(&format!(" {tool}@")) || run.contains(&format!(" {tool}["))
}

fn set_digest(job: &mut Job, digest: &str) -> Result<(), Box<dyn Error>> {
    for step in &mut job.steps {
        if let (Some(StepRole::ToolsCacheIdentity), StepKind::Action { with, .. }) =
            (&step.role, &mut step.kind)
        {
            with.insert(
                TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
                digest.to_owned(),
            );
            return Ok(());
        }
    }
    Err("job has no tools identity step".into())
}

fn crate_id(jobs: &BTreeMap<String, Job>) -> Result<String, Box<dyn Error>> {
    jobs.iter()
        .find(|(id, job)| id.starts_with(CRATE_JOB_ID_PREFIX) && is_lane_job(id, job))
        .map(|(id, _)| id.clone())
        .ok_or_else(|| "no rust crate job".into())
}

fn foreign_digest(jobs: &BTreeMap<String, Job>, own: &str) -> Result<String, Box<dyn Error>> {
    jobs.values()
        .filter_map(identity_digest)
        .find(|digest| *digest != own)
        .map(str::to_owned)
        .ok_or_else(|| "tree has no second tools digest".into())
}

#[test]
fn rust_jobs_in_one_lane_share_one_union_tool_set() -> Result<(), Box<dyn Error>> {
    let jobs = lane_jobs()?;
    let lane: Vec<(&String, &Job)> = jobs
        .iter()
        .filter(|(id, job)| is_lane_job(id, job))
        .collect();
    let ids: BTreeSet<&str> = lane.iter().map(|(id, _)| id.as_str()).collect();
    assert!(ids.contains(LEDGER_CLI_JOB_ID), "{ids:?}");
    assert!(
        ids.iter().any(|id| id.starts_with(CRATE_JOB_ID_PREFIX)),
        "{ids:?}"
    );
    assert!(
        lane.iter()
            .any(|(_, job)| job.producer_kind() == Some(ProducerKind::StateProducer)),
        "{ids:?}"
    );
    let mut digests = BTreeMap::<&str, BTreeSet<&str>>::new();
    for (id, job) in &lane {
        let digest = identity_digest(job).ok_or_else(|| format!("{id} has no tools identity"))?;
        digests
            .entry(job.runs_on.as_str())
            .or_default()
            .insert(digest);
    }
    for (runs_on, set) in &digests {
        assert_eq!(set.len(), 1, "{runs_on}: {set:?}");
    }
    for (id, job) in &lane {
        let run = install_run(job)?;
        for tool in LANE_TOOLS {
            assert!(installs(&run, tool), "{id} does not install {tool}: {run}");
        }
    }
    let tofu = jobs
        .iter()
        .find(|(id, _)| id.starts_with(TOFU_JOB_ID_PREFIX))
        .ok_or("no tofu job")?
        .1;
    assert!(!installs(&install_run(tofu)?, "rust"));
    Ok(())
}

#[test]
fn w7_refuses_two_tools_identities_in_one_lane() -> Result<(), Box<dyn Error>> {
    let mut jobs = lane_jobs()?;
    assert_eq!(waste_findings(&jobs), Vec::new());
    let id = crate_id(&jobs)?;
    let own = identity_digest(&jobs[&id])
        .ok_or("crate job has no identity")?
        .to_owned();
    let other = foreign_digest(&jobs, &own)?;
    let runs_on = jobs[&id].runs_on.clone();
    set_digest(jobs.get_mut(&id).ok_or("job vanished")?, &other)?;
    let found = waste_findings(&jobs);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].rule(), WasteRule::SplitToolsIdentity);
    let message = found[0].to_string();
    for name in [
        id.as_str(),
        LEDGER_CLI_JOB_ID,
        runs_on.as_str(),
        &own[..8],
        &other[..8],
    ] {
        assert!(message.contains(name), "{name} missing from {message}");
    }
    set_digest(jobs.get_mut(&id).ok_or("job vanished")?, "not-a-digest")?;
    let found = waste_findings(&jobs);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        matches!(&found[0], WasteFinding::ToolsIdentity(_)) && found[0].to_string().contains(&id),
        "{found:?}"
    );
    Ok(())
}

#[test]
fn a_rust_job_in_another_lane_keeps_its_own_identity() -> Result<(), Box<dyn Error>> {
    let mut jobs = lane_jobs()?;
    let id = crate_id(&jobs)?;
    let own = identity_digest(&jobs[&id])
        .ok_or("crate job has no identity")?
        .to_owned();
    let other = foreign_digest(&jobs, &own)?;
    let mut same_lane = jobs[&id].clone();
    set_digest(&mut same_lane, &other)?;
    let mut other_lane = same_lane.clone();
    other_lane.runs_on = OTHER_LANE_RUNS_ON.to_owned();
    for step in &mut other_lane.steps {
        if let (Some(StepRole::ToolsCacheIdentity), StepKind::Action { uses, .. }) =
            (&step.role, &mut step.kind)
        {
            OTHER_LANE_USES.clone_into(uses);
        }
    }
    jobs.insert("rust-other-lane".to_owned(), other_lane);
    assert_eq!(waste_findings(&jobs), Vec::new());
    jobs.insert("rust-same-lane".to_owned(), same_lane);
    let found = waste_findings(&jobs);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].rule(), WasteRule::SplitToolsIdentity);
    Ok(())
}

#[test]
fn w7_exempts_the_plan_and_the_sources_producer() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_for(false, WorkflowPolicy::ConsumerV1)?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let crate_digests: BTreeSet<&str> = jobs
        .iter()
        .filter(|(id, job)| id.starts_with(CRATE_JOB_ID_PREFIX) && !is_tools_producer(job))
        .filter_map(|(_, job)| identity_digest(job))
        .collect();
    assert!(!crate_digests.is_empty());
    for exempt in [PLAN_JOB_ID, SOURCES_PRODUCER_JOB_ID] {
        let digest = jobs
            .get(exempt)
            .and_then(identity_digest)
            .ok_or_else(|| format!("{exempt} has no tools identity"))?;
        assert!(
            !crate_digests.contains(digest),
            "{exempt} shares the crate identity, so the exemption proves nothing"
        );
    }
    assert_eq!(waste_findings(&jobs), Vec::new());
    Ok(())
}
