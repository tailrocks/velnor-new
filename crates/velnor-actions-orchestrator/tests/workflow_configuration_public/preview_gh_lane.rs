// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use velnor_actions_contract::{StepKind, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::workflow_configuration_fixture::write_root_cargo_lock;

fn configure_verifier_fixture(
    root: &std::path::Path,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let manifest = root.join("crates/maintenance/Cargo.toml");
    let content = fs::read_to_string(&manifest)?;
    fs::write(
        &manifest,
        content.replace(
            "name = \"maintenance\"",
            "name = \"velnor-actions-workflow-renderer\"",
        ),
    )?;
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        let validator = root.join("crates/validator");
        fs::create_dir_all(validator.join("src"))?;
        fs::write(
            validator.join("Cargo.toml"),
            "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;
        fs::write(validator.join("src/lib.rs"), "pub fn validator() {}\n")?;
        let workspace = root.join("Cargo.toml");
        let root_manifest = fs::read_to_string(&workspace)?;
        fs::write(
            &workspace,
            root_manifest.replace(
                "members = [\"crates/maintenance\"]",
                "members = [\"crates/maintenance\", \"crates/validator\"]",
            ),
        )?;
        super::workflow_configuration_fixture::write_nextest_task(&validator)?;
        write_root_cargo_lock(
            root,
            &[
                "demo",
                "velnor-actions-cli",
                "velnor-actions-workflow-renderer",
            ],
        )?;
    } else {
        write_root_cargo_lock(root, &["demo", "velnor-actions-workflow-renderer"])?;
    }
    Ok(())
}

fn assert_verifier_follows_validator(owner_lane: &str) -> Result<(), Box<dyn Error>> {
    let install = owner_lane
        .find("name: Prepare pinned tools")
        .ok_or("missing shared tools")?;
    let verify = owner_lane
        .find("name: Verify jactionlint")
        .ok_or("missing jactionlint verification")?;
    let gh = owner_lane
        .find("name: Prepare pinned attestation verifier")
        .ok_or("missing runtime verifier")?;
    assert!(
        install < verify && verify < gh,
        "jactionlint verification must precede runtime Gh installation"
    );
    Ok(())
}

#[test]
fn gh_verifier_is_installed_only_for_its_owned_renderer_lane() -> Result<(), Box<dyn Error>> {
    for policy in [
        WorkflowPolicy::VelnorRepositoryV1,
        WorkflowPolicy::ConsumerV1,
    ] {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        configure_verifier_fixture(repo.path(), policy)?;
        let prepared = prepare(repo.path())?;
        assert!(
            prepared.lock_audit_blocking.is_empty(),
            "real generator cannot run: {:?}",
            prepared.lock_audit_blocking
        );
        let rendered = render_staged_tree(&prepared)?;
        let workflow = rendered.get(WORKFLOW_PATH).ok_or("missing workflow")?;
        let owner_lane = rendered_job_block(workflow, "rust-velnor-actions-workflow-renderer")?;
        let other = rendered_job_block(workflow, "rust-demo")?;
        let pinned = format!(
            "gh@{}",
            prepared
                .catalog
                .version(velnor_actions_mise::PinnedTool::Gh)
        );
        assert_eq!(
            owner_lane.contains(&pinned),
            policy == WorkflowPolicy::VelnorRepositoryV1,
            "renderer lane: {policy:?}"
        );
        assert!(
            !other.contains(&pinned),
            "unrelated lane installed gh: {policy:?}: {other}"
        );
        if policy == WorkflowPolicy::VelnorRepositoryV1 {
            assert_verifier_follows_validator(owner_lane)?;
            let state = rendered
                .get(".github/workflows/state-seal.yml")
                .ok_or("missing state producer workflow")?;
            for line in state
                .lines()
                .filter(|line| line.starts_with("  tools-producer-") && line.ends_with(':'))
            {
                let id = line.trim().trim_end_matches(':');
                assert!(
                    !rendered_job_block(state, id)?.contains(&pinned),
                    "producer installed runtime-only gh"
                );
            }
            let mut ir = prepared.workflow.ir.clone();
            ir.validate()?;
            let lane = ir
                .jobs
                .get_mut("rust-velnor-actions-workflow-renderer")
                .ok_or("missing renderer lane")?;
            let step = lane
                .steps
                .iter_mut()
                .find(|step| step.role == Some(StepRole::PrepareRuntimeTools))
                .ok_or("missing runtime step")?;
            let StepKind::Shell { run, .. } = &mut step.kind else {
                return Err("runtime step is not shell".into());
            };
            *run = [
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "install",
                "rust@1.99.1",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            assert!(
                ir.validate().is_err(),
                "runtime role hid a compiler installation"
            );
        }
    }
    Ok(())
}
