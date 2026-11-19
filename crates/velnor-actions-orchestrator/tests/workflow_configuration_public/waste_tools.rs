// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::{Job, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{WasteRule, finalized_jobs, prepare};
use velnor_actions_workflow_renderer::steps::check_mbx_gating;

use super::one_job_per_crate::mbx_repo_for;
use super::tools_producers::identity_digest;
use super::waste_graph::of;
use super::workflow_configuration_fixture;

fn lane_jobs_with_catalog() -> Result<(BTreeMap<String, Job>, ToolCatalog), Box<dyn Error>> {
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
    let prepared = prepare(root)?;
    Ok((finalized_jobs(&prepared)?, prepared.emission_catalog()))
}

#[test]
fn a_prebuilt_image_job_shares_the_lane_identity() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_for(true, WorkflowPolicy::VelnorRepositoryV1)?;
    let root = repo.path();
    fs::write(root.join("src/main.rs"), "fn main() {}\n")?;
    workflow_configuration_fixture::write_root_cargo_lock(root, &["demo"])?;
    fs::create_dir_all(root.join("docker"))?;
    fs::write(
        root.join("docker/Dockerfile"),
        "FROM scratch\nCOPY app /app\n",
    )?;
    let mut config = fs::read_to_string(root.join(".velnor/config.toml"))?;
    config.push_str(
        "\n[[images]]\nid = \"app\"\nrepository = \"ghcr.io/acme/app\"\ndockerfile = \"docker/Dockerfile\"\ncontext = \"docker\"\nplatforms = [\"linux_amd64\"]\n\n[images.prebuilt]\nkind = \"cargo_bin\"\npackage = \"demo\"\nbin = \"demo\"\nstage = \"docker/app\"\n",
    );
    fs::write(root.join(".velnor/config.toml"), config)?;
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    let jobs = finalized_jobs(&prepare(root)?)?;
    assert_eq!(of(&jobs, WasteRule::SplitToolsIdentity), Vec::new());
    let image = jobs.get("image-app-amd64").ok_or("no image job")?;
    let image_digest = identity_digest(image).ok_or("image job has no tools identity")?;
    let lane = jobs
        .get(LEDGER_CLI_JOB_ID)
        .ok_or("no ledger-cli lane job")?;
    assert_eq!(Some(image_digest), identity_digest(lane));
    assert_eq!(image.runs_on, lane.runs_on);
    Ok(())
}

#[test]
fn an_install_only_mbx_selector_must_equal_the_catalog_version() -> Result<(), Box<dyn Error>> {
    let (jobs, catalog) = lane_jobs_with_catalog()?;
    let reference = catalog.version(PinnedTool::MrBoxington).to_owned();
    let (id, job) = jobs
        .iter()
        .find(|(_, job)| {
            job.steps.iter().any(|step| {
                step.role == Some(StepRole::PreparePinnedTools)
                    && matches!(&step.kind, StepKind::Shell { run, .. } if run.iter().any(|word| word.contains("mr-boxington@")))
            })
        })
        .ok_or("no job installs mr-boxington through the pinned tools step")?;
    let one = BTreeMap::from([(id.clone(), job.clone())]);
    check_mbx_gating(&one, &BTreeMap::new(), &catalog)?;
    let mut stale = job.clone();
    for step in &mut stale.steps {
        if let (Some(StepRole::PreparePinnedTools), StepKind::Shell { run, .. }) =
            (&step.role, &mut step.kind)
        {
            for word in run.iter_mut() {
                *word = word.replace(&reference, "0.0.1");
            }
        }
    }
    let message = check_mbx_gating(
        &BTreeMap::from([(id.clone(), stale)]),
        &BTreeMap::new(),
        &catalog,
    )
    .err()
    .ok_or("a stale mr-boxington selector was accepted")?
    .to_string();
    assert!(message.contains("mbx_mise_selector_forbidden"), "{message}");
    assert!(message.contains(id.as_str()), "{message}");
    Ok(())
}
