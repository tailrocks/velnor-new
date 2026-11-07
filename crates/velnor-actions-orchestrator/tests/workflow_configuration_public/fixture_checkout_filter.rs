// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::waste_tools::lane_jobs;
use std::error::Error;
use velnor_actions_contract::{StepKind, workflow::step_identity::FIXTURE_CHECKOUT_BLOB_FILTER};

#[test]
fn only_owning_fixture_jobs_fetch_all_blobs_without_persisting_credentials()
-> Result<(), Box<dyn Error>> {
    let jobs = lane_jobs()?;
    let mut owners = 0;
    for job in jobs.values() {
        let owns = job
            .steps
            .iter()
            .any(|step| step.name == "Stage isolated GitHub CLI test tools");
        let Some(checkout) = job.steps.iter().find(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))) else { continue; };
        let StepKind::Action { with, env, .. } = &checkout.kind else {
            return Err("checkout missing".into());
        };
        if owns {
            owners += 1;
            assert_eq!(
                with.get("filter").map(String::as_str),
                Some(FIXTURE_CHECKOUT_BLOB_FILTER),
                "{}",
                job.display_name
            );
            assert_eq!(
                with.get("persist-credentials").map(String::as_str),
                Some("false")
            );
            assert!(env.is_empty());
        } else {
            assert!(!with.contains_key("filter"), "{}", job.display_name);
        }
    }
    assert_eq!(owners, 2);
    Ok(())
}

#[test]
fn private_partial_checkout_keeps_skipped_blobs_without_credentials() -> Result<(), Box<dyn Error>>
{
    let status = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-B"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/workflow_configuration_public/fixture_checkout_filter.py"
        ))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn finalized_sparse_owners_keep_guard_and_qualified_filter() -> Result<(), Box<dyn Error>> {
    use super::{non_inputs_checkout_support::rust_repo, workflow_configuration_fixture};
    use velnor_actions_contract::WorkflowPolicy;
    use velnor_actions_orchestrator::{finalized_jobs, prepare};
    use velnor_actions_workflow_renderer::non_input_checkout::NON_INPUT_GUARD_STEP;
    let repo = rust_repo(WorkflowPolicy::VelnorRepositoryV1, Some("docs/**"))?;
    let root = repo.path();
    for (dir, name) in [
        ("crates/cli", "velnor-actions-cli"),
        ("crates/orchestrator", "velnor-actions-orchestrator"),
        ("crates/mise", "velnor-actions-mise"),
    ] {
        workflow_configuration_fixture::add_independent_mbx_package(root, dir, name)?;
        workflow_configuration_fixture::write_nextest_task(&root.join(dir))?;
    }
    std::fs::write(
        root.join("mise.toml"),
        format!(
            "{}\n[tasks.fixture-check]\nrun = \"true\"\n",
            include_str!("../../../../mise.toml")
        ),
    )?;
    std::fs::write(
        root.join("mise.lock"),
        include_str!("../../../../mise.lock"),
    )?;
    let config_path = root.join(".velnor/config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    std::fs::write(
        &config_path,
        format!(
            "{config}\n[[checks]]\nid = \"with-blob-closure\"\ntask = \"fixture-check\"\ninputs = [\"crates/demo/Cargo.toml\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
        ),
    )?;
    let jobs = finalized_jobs(&prepare(root)?)?;
    let mut owners = 0;
    let mut named = 0;
    for job in jobs.values().filter(|job| {
        job.display_name.contains("with-blob-closure") || job.display_name.contains("unit")
    }) {
        let StepKind::Action { with, env, .. } = &job.steps[0].kind else {
            return Err("named check checkout missing".into());
        };
        assert_eq!(
            with.get("filter").map(String::as_str),
            Some(FIXTURE_CHECKOUT_BLOB_FILTER)
        );
        assert_eq!(
            with.get("sparse-checkout").map(String::as_str),
            Some("/*\n!/docs/**")
        );
        assert_eq!(
            with.get("persist-credentials").map(String::as_str),
            Some("false")
        );
        assert!(env.is_empty());
        assert_eq!(with.get("fetch-depth").map(String::as_str), Some("0"));
        assert_eq!(job.steps[1].name, NON_INPUT_GUARD_STEP);
        named += 1;
    }
    assert_eq!(named, 2);
    for job in jobs.values() {
        let owns = job
            .steps
            .iter()
            .any(|step| step.name == "Stage isolated GitHub CLI test tools");
        if !owns {
            continue;
        }
        owners += 1;
        let StepKind::Action { with, env, .. } = &job.steps[0].kind else {
            return Err("checkout missing".into());
        };
        assert_eq!(
            with.get("filter").map(String::as_str),
            Some(FIXTURE_CHECKOUT_BLOB_FILTER)
        );
        assert_eq!(
            with.get("sparse-checkout").map(String::as_str),
            Some("/*\n!/docs/**")
        );
        assert_eq!(
            with.get("persist-credentials").map(String::as_str),
            Some("false")
        );
        assert!(env.is_empty());
        assert_eq!(job.steps[1].name, NON_INPUT_GUARD_STEP);
    }
    assert_eq!(owners, 2);
    Ok(())
}
