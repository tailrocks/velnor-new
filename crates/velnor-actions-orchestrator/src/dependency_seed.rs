//! Dependency seed jobs write MBX bundles for crate jobs.
//!
//! Cache decision 2 defines one seed job for each MBX Cargo workspace.
//! On a trusted push to the default branch, it compiles test executables,
//! runs Clippy, and builds documentation for the whole workspace.
//! After these commands succeed, it exports and saves an MBX bundle.
//! Crate jobs import that bundle and do not write it.
//! The seed job uploads no task report and is not part of `Required`.
//! Hard decision 1 requires one job per crate. It applies only to crate jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, Step, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::steps::{MbxTransport, mbx_producer_mark_step, shell_step};

use super::CrateJobInputs;
use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::discover::Discovery;

/// Identify the seed job for the workspace at the repository root.
pub(crate) const SEED_JOB_ID: &str = "dependency-seed";
/// Run the seed job only for a protected push to the default branch.
pub(crate) const SEED_JOB_CONDITION: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

/// Identify one MBX Cargo workspace that needs a seed job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeedWorkspace {
    /// This path identifies the workspace root relative to the checkout.
    /// An empty string identifies the repository root.
    pub(crate) root: String,
    /// This value is true when any workspace task uses Nextest.
    pub(crate) nextest: bool,
}

/// Collect distinct Cargo workspaces that contain a runnable MBX task.
pub(crate) fn workspaces_for_discovery(
    discovery: &Discovery,
    fetch_roots: &[String],
) -> Vec<SeedWorkspace> {
    let mut roots = BTreeMap::<String, bool>::new();
    for task in &discovery.proposals {
        if !super::is_runnable(task) || !super::is_mbx(task) {
            continue;
        }
        let root =
            crate::source_prep::workspace_root_for_manifest(&task.identity.unit_path, fetch_roots)
                .to_owned();
        *roots.entry(root).or_default() |= super::is_nextest(task);
    }
    roots
        .into_iter()
        .map(|(root, nextest)| SeedWorkspace { root, nextest })
        .collect()
}

/// Return the seed job ID for one workspace root.
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
/// This function returns a contract or render error if the root is unsafe or a step fails validation.
pub(crate) fn seed_jobs(
    input: &CrateJobInputs<'_>,
    workspaces: &[SeedWorkspace],
    private: Option<&PrivateWiring>,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let job_ids = unique_seed_job_ids(workspaces);
    workspaces
        .iter()
        .zip(job_ids)
        .map(|(workspace, job_id)| Ok((job_id, seed_job(input, workspace, private)?)))
        .collect()
}

pub(crate) fn unique_seed_job_ids(workspaces: &[SeedWorkspace]) -> Vec<String> {
    let job_ids: Vec<_> = workspaces
        .iter()
        .map(|workspace| seed_job_id(&workspace.root))
        .collect();
    if job_ids.iter().collect::<BTreeSet<_>>().len() == job_ids.len()
        && job_ids
            .iter()
            .all(|job_id| velnor_actions_contract::validate_job_id(job_id).is_ok())
    {
        return job_ids;
    }
    workspaces
        .iter()
        .map(|workspace| {
            if workspace.root.is_empty() {
                SEED_JOB_ID.to_owned()
            } else {
                format!("{SEED_JOB_ID}-{}", encode_seed_root(&workspace.root))
            }
        })
        .collect()
}

fn encode_seed_root(root: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(root.len().saturating_mul(2));
    for byte in root.bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
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
    steps.extend(crate::source_prep::fetch_steps_for_crate_with_all_features(
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

/// Build the three fixed lanes of one workspace in the same order as a crate job.
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
