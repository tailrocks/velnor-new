// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::workflow::crate_job::{GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX};
use velnor_actions_contract::workflow::group_ledger::EMIT_GROUP_LEDGER_OP;
use velnor_actions_contract::{
    CacheAccess, Job, Step, StepId, StepKind, StepRole, TOFU_JOB_ID_PREFIX, WorkflowPolicy,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare};
use velnor_actions_workflow_renderer::INTERNAL_OP_ENV;

use super::support::config_with_timeouts;
use super::tools_producers::{POLICIES, is_upload};
use super::workflow_configuration_fixture;

pub(super) struct Rendered {
    pub(super) name: String,
    pub(super) policy: WorkflowPolicy,
    pub(super) jobs: BTreeMap<String, Job>,
}

pub(super) fn render(
    name: &str,
    repo: &TempDir,
    policy: WorkflowPolicy,
) -> Result<Rendered, Box<dyn Error>> {
    let prepared = prepare(repo.path())?;
    Ok(Rendered {
        name: name.to_owned(),
        policy,
        jobs: finalized_jobs(&prepared)?,
    })
}

pub(super) fn mbx_repo(tofu: bool) -> Result<TempDir, Box<dyn Error>> {
    mbx_repo_for(tofu, WorkflowPolicy::ConsumerV1)
}

pub(super) fn mbx_repo_for(tofu: bool, policy: WorkflowPolicy) -> Result<TempDir, Box<dyn Error>> {
    mbx_repo_with_crate_timeout(tofu, policy, None)
}

pub(super) fn mbx_repo_with_crate_timeout(
    tofu: bool,
    policy: WorkflowPolicy,
    crate_timeout: Option<u16>,
) -> Result<TempDir, Box<dyn Error>> {
    let mut config = config_with_timeouts(15, crate_timeout)?;
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        config = config.replacen(
            "name = \"CI\"\n",
            "name = \"CI\"\npolicy = \"velnor-repository-v1\"\n",
            1,
        );
    }
    if tofu {
        config.push_str("\n[stacks.tofu]\nroots = [\"infra\"]\n");
    }
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    workflow_configuration_fixture::add_independent_mbx_package(root, "crates/nested", "nested")?;
    workflow_configuration_fixture::write_nextest_task(root)?;
    workflow_configuration_fixture::write_nextest_task(&root.join("crates/nested"))?;
    if tofu {
        fs::create_dir_all(root.join("infra"))?;
        fs::write(
            root.join("infra/main.tf"),
            "terraform {\n  required_providers {\n    random = {\n      source = \"hashicorp/random\"\n      version = \"3.6.0\"\n    }\n  }\n}\n",
        )?;
        fs::write(
            root.join("infra/.terraform.lock.hcl"),
            "provider \"registry.opentofu.org/hashicorp/random\" {\n  version = \"3.6.0\"\n  hashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n",
        )?;
    }
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        workflow_configuration_fixture::git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            root,
        )?;
        fs::write(
            root.join(".velnor/generator.lock"),
            include_str!("../../../../.velnor/generator.lock"),
        )?;
        fs::write(
            root.join(".velnor/controller-approvals.json"),
            include_str!("../../../../.velnor/controller-approvals.json"),
        )?;
        workflow_configuration_fixture::write_root_cargo_lock(root, &["demo"])?;
    }
    Ok(repo)
}

pub(super) fn group_fixtures() -> Result<Vec<Rendered>, Box<dyn Error>> {
    POLICIES.into_iter().map(tofu_fixture_for).collect()
}

pub(super) fn tofu_fixture() -> Result<Rendered, Box<dyn Error>> {
    tofu_fixture_for(WorkflowPolicy::ConsumerV1)
}

pub(super) fn tofu_fixture_for(policy: WorkflowPolicy) -> Result<Rendered, Box<dyn Error>> {
    render(
        &format!("tofu-{policy:?}"),
        &mbx_repo_for(true, policy)?,
        policy,
    )
}

pub(super) fn crate_jobs(jobs: &BTreeMap<String, Job>) -> Vec<(&String, &Job)> {
    jobs.iter()
        .filter(|(id, job)| id.starts_with(TOFU_JOB_ID_PREFIX) && job.producer_kind().is_none())
        .collect()
}

pub(super) fn is_emit(step: &Step) -> bool {
    match &step.kind {
        StepKind::Internal { operation, .. } => operation == EMIT_GROUP_LEDGER_OP,
        StepKind::Shell { env, .. } => env
            .get(INTERNAL_OP_ENV)
            .is_some_and(|operation| operation == EMIT_GROUP_LEDGER_OP),
        _ => false,
    }
}

fn is_crate_report_upload(step: &Step) -> bool {
    is_upload(step)
        && matches!(&step.kind, StepKind::Action { with, .. }
            if with.get("name").is_some_and(|name| name.starts_with("velnor-crate-")))
}

fn assert_emits_once_before_upload(fixture: &Rendered) {
    let Rendered {
        name, jobs, policy, ..
    } = fixture;
    let crates = crate_jobs(jobs);
    assert!(!crates.is_empty(), "{name}: no crate jobs");
    for (id, job) in crates {
        let emits: Vec<usize> = (0..job.steps.len())
            .filter(|at| is_emit(&job.steps[*at]))
            .collect();
        let uploads: Vec<usize> = (0..job.steps.len())
            .filter(|at| is_crate_report_upload(&job.steps[*at]))
            .collect();
        assert_eq!((emits.len(), uploads.len()), (1, 1), "{name}: {id}");
        assert!(emits[0] < uploads[0], "{name}: {id} emits after its upload");
        let internal = matches!(job.steps[emits[0]].kind, StepKind::Internal { .. });
        assert_eq!(
            internal,
            *policy == WorkflowPolicy::ConsumerV1,
            "{name}: {id} emit form"
        );
        let expected_condition = match policy {
            WorkflowPolicy::ConsumerV1 => "always()".to_owned(),
            WorkflowPolicy::VelnorRepositoryV1 => format!(
                "always() && steps.{}.outcome == 'success'",
                StepId::LedgerCliVerify.as_str()
            ),
        };
        assert_eq!(
            job.steps[emits[0]].condition.as_deref(),
            Some(expected_condition.as_str()),
            "{name}: {id} emit condition"
        );
        assert_eq!(job.steps[emits[0]].role, None::<StepRole>);
    }
}

#[test]
fn each_crate_job_emits_one_group_ledger_before_upload() -> Result<(), Box<dyn Error>> {
    for fixture in group_fixtures()? {
        assert_emits_once_before_upload(&fixture);
    }
    Ok(())
}

#[test]
fn tofu_group_stays_one_literal_read_job_without_a_provider_save() -> Result<(), Box<dyn Error>> {
    let Rendered { jobs, .. } = tofu_fixture()?;
    let tofu: Vec<(&String, &Job)> = jobs
        .iter()
        .filter(|(id, _)| id.starts_with(TOFU_JOB_ID_PREFIX))
        .collect();
    assert_eq!(tofu.len(), 1, "{:?}", jobs.keys().collect::<Vec<_>>());
    let (id, job) = tofu[0];
    assert!(!id.ends_with(GROUP_READER_SUFFIX) && !id.ends_with(GROUP_WRITER_SUFFIX));
    assert_eq!(
        job.cache_access,
        CacheAccess::Read,
        "{id} is not cache-mode read"
    );
    assert!(
        job.steps
            .iter()
            .all(|step| step.role != Some(StepRole::TofuProvidersSave)),
        "{id} saves providers"
    );
    Ok(())
}
