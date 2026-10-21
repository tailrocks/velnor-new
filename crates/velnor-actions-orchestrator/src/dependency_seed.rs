//! Dependency-seed jobs: the only writer of the compiled-dependency bundle.
//!
//! Cache decision 2. One non-crate job per MBX Cargo workspace compiles the
//! three fixed lanes (test executables, Clippy, documentation) for the whole
//! workspace on a trusted default-branch push and saves the opaque
//! `mbx cache export` bundle once the compile succeeded. Crate jobs restore
//! that bundle read-only. The seed job runs no checks, uploads no report and
//! is never part of `Required`, so hard decision 1 (one job per crate) governs
//! crate jobs only.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::steps::{MbxTransport, mbx_producer_mark_step, shell_step};

use super::CrateJobInputs;
use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;

/// Job id of the seed for the repository-root workspace.
pub(crate) const SEED_JOB_ID: &str = "dependency-seed";
/// Job-level condition: only a protected push to the default branch seeds.
pub(crate) const SEED_JOB_CONDITION: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

/// One MBX Cargo workspace that needs a seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeedWorkspace {
    /// Workspace root relative to the checkout (`""` for the repository root).
    pub(crate) root: String,
    /// True when any task of the workspace runs on Nextest.
    pub(crate) nextest: bool,
}

/// Seed job id for one workspace root.
pub(crate) fn seed_job_id(root: &str) -> String {
    if root.is_empty() {
        SEED_JOB_ID.to_owned()
    } else {
        format!("{SEED_JOB_ID}-{}", root.replace('/', "-"))
    }
}

/// Build one seed job per workspace.
///
/// # Errors
///
/// Returns contract or render errors for an unsafe root or rejected step.
pub(crate) fn seed_jobs(
    input: &CrateJobInputs<'_>,
    workspaces: &[SeedWorkspace],
    private: Option<&PrivateWiring>,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    workspaces
        .iter()
        .map(|workspace| {
            Ok((
                seed_job_id(&workspace.root),
                seed_job(input, workspace, private)?,
            ))
        })
        .collect()
}

fn seed_job(
    input: &CrateJobInputs<'_>,
    workspace: &SeedWorkspace,
    private: Option<&PrivateWiring>,
) -> Result<Job, OrchestratorError> {
    let catalog = input.catalog;
    crate::source_prep::validate_root(&workspace.root)?;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        true,
        workspace.nextest,
        false,
        false,
    )?);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.push(crate::workflow::verify_rust_manifest_step(catalog)?);
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
        steps.extend(private.steps_for_roots(&roots)?);
    }
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog, &roots, false,
    )?);
    steps.extend(lane_steps(catalog, workspace)?);
    steps.push(mbx_producer_mark_step()?);
    Ok(Job {
        services: Vec::new(),
        display_name: display_name(&workspace.root),
        runs_on: input.label.to_owned(),
        check_runner: None,
        timeout_minutes: input.timeout,
        needs: vec![velnor_actions_workflow_renderer::render::PLAN_JOB_ID.to_owned()],
        condition: Some(SEED_JOB_CONDITION.to_owned()),
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

/// The three fixed lane builds of one workspace, in the lane order of a crate job.
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
