// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::steps::{MbxTransport, mbx_producer_mark_step, shell_step};

use super::CrateJobInputs;
use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeedWorkspace {
    pub(crate) root: String,
    pub(crate) nextest: bool,
}

pub(crate) fn seed_jobs(
    input: &CrateJobInputs<'_>,
    workspaces: &[SeedWorkspace],
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let job_ids = unique_seed_job_ids(workspaces);
    workspaces
        .iter()
        .zip(job_ids)
        .map(|(workspace, job_id)| Ok((job_id, seed_job(input, workspace, private, timeout)?)))
        .collect()
}

pub(crate) fn unique_seed_job_ids(workspaces: &[SeedWorkspace]) -> Vec<String> {
    velnor_actions_workflow_renderer::native_authority::unique_dependency_seed_job_ids(
        &workspaces
            .iter()
            .map(|workspace| workspace.root.clone())
            .collect::<Vec<_>>(),
    )
}

fn seed_job(
    input: &CrateJobInputs<'_>,
    workspace: &SeedWorkspace,
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Job, OrchestratorError> {
    let catalog = input.catalog;
    crate::source_prep::validate_root(&workspace.root)?;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.push(crate::workflow::verify_restored_rust_step(catalog));
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        true,
        workspace.nextest,
        false,
        false,
    )?);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.push(crate::workflow::verify_rust_manifest_step(catalog));
    steps.extend(crate::source_cache::restore_step_for_crate(
        input.fetch_roots,
        true,
    )?);
    let working_dir = (!workspace.root.is_empty()).then_some(workspace.root.as_str());
    steps.extend(crate::mbx_preflight::steps_for_catalog(
        catalog,
        MbxTransport::SeedWrite,
        working_dir,
    )?);
    let roots = vec![workspace.root.clone()];
    if let Some(private) = private {
        let private_steps = private.steps_for_roots(&roots)?;
        if !private_steps.is_empty() {
            let acquire = input.acquire.ok_or_else(|| OrchestratorError::Contract {
                problem: "private_sources_seed_without_acquire".to_owned(),
            })?;
            steps.insert(1, acquire.clone());
        }
        steps.extend(private_steps);
    }
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog, &roots, false,
    )?);
    steps.extend(lane_steps(catalog, workspace)?);
    steps.push(mbx_producer_mark_step()?);
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: display_name(&workspace.root),
        runs_on: input.label.to_owned(),
        check_runner: None,
        timeout_minutes: timeout,
        needs: vec![velnor_actions_workflow_renderer::render::PLAN_JOB_ID.to_owned()],
        condition: Some(
            velnor_actions_contract::workflow::DEPENDENCY_SEED_JOB_CONDITION.to_owned(),
        ),
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    })
}

fn display_name(root: &str) -> String {
    if root.is_empty() {
        "Dependency seed".to_owned()
    } else {
        format!("Dependency seed / {root}")
    }
}

fn lane_steps(
    catalog: &ToolCatalog,
    workspace: &SeedWorkspace,
) -> Result<Vec<Step>, OrchestratorError> {
    let manifest = if workspace.root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{}/Cargo.toml", workspace.root)
    };
    let lane = |name: &str| {
        if workspace.root.is_empty() {
            format!("target/{name}")
        } else {
            format!("{}/target/{name}", workspace.root)
        }
    };
    let mut test_build: Vec<&str> = if workspace.nextest {
        vec!["nextest", "list", "--list-type", "binaries-only"]
    } else {
        vec!["test", "--no-run"]
    };
    test_build.extend([
        "--locked",
        "--offline",
        "--manifest-path",
        &manifest,
        "--workspace",
    ]);
    let clippy_lane = lane("velnor-clippy");
    let doc_lane = lane("velnor-doc");
    let lanes: [(&str, Vec<&str>); 3] = [
        ("Compile test executables (seed)", test_build),
        (
            "Compile Clippy lane (seed)",
            vec![
                "clippy",
                "--locked",
                "--offline",
                "--manifest-path",
                &manifest,
                "--workspace",
                "--all-targets",
                "--target-dir",
                &clippy_lane,
            ],
        ),
        (
            "Compile documentation lane (seed)",
            vec![
                "doc",
                "--locked",
                "--offline",
                "--manifest-path",
                &manifest,
                "--workspace",
                "--no-deps",
                "--target-dir",
                &doc_lane,
            ],
        ),
    ];
    let mut tools = vec![PinnedTool::Rust];
    if workspace.nextest {
        tools.push(PinnedTool::Nextest);
    }
    let base_env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    lanes
        .into_iter()
        .map(|(name, args)| {
            let argv = crate::vectors::exec_argv(tools.clone(), "mbx", &args, catalog)?;
            let mut step = shell_step(name, argv, base_env.clone())?;
            step.role = Some(StepRole::MbxSeedBuild);
            Ok(step)
        })
        .collect()
}
