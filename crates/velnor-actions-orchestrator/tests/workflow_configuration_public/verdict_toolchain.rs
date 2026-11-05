// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use serde_json::Value;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::workflow_configuration_fixture::{config_with_verification_branch, make_repo};

fn workflow(rust: Option<&str>) -> Result<(String, Value), Box<dyn Error>> {
    let config = config_with_verification_branch(None)
        + "plan_timeout_minutes = 15\ncrate_timeout_minutes = 15\n";
    let repo = make_repo(&config)?;
    if let Some(version) = rust {
        fs::write(
            repo.path().join("mise.toml"),
            format!("[tools]\nrust = \"{version}\"\n"),
        )?;
    }
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let text = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    Ok((text.to_owned(), serde_saphyr::from_str(text)?))
}

#[test]
fn verdict_does_not_copy_declared_rust_versions() -> Result<(), Box<dyn Error>> {
    let version = velnor_actions_mise::RUST_VERSION;
    let (text, document) = workflow(Some(version))?;
    assert!(!text.contains(version), "workflow copied Rust {version}");
    let jobs = document["jobs"].as_object().ok_or("missing jobs")?;
    let required = jobs.get("required").ok_or("missing verdict")?;
    assert!(!required.to_string().contains("RUSTUP_TOOLCHAIN"));
    let rust = jobs.get("rust-demo").ok_or("missing Rust job")?;
    let rust = rust.to_string();
    assert!(rust.contains("VELNOR_TOOL_RUST"));
    assert!(rust.contains("RUSTUP_TOOLCHAIN"));
    assert!(rust.contains("Resolve project tool versions"));
    Ok(())
}

#[test]
fn undeclared_rust_remains_pinned_only_in_rust_jobs() -> Result<(), Box<dyn Error>> {
    let (_, document) = workflow(None)?;
    let jobs = document["jobs"].as_object().ok_or("missing jobs")?;
    let required = jobs.get("required").ok_or("missing verdict")?;
    assert!(!required.to_string().contains("RUSTUP_TOOLCHAIN"));
    let rust = jobs.get("rust-demo").ok_or("missing Rust job")?;
    assert!(rust.to_string().contains(velnor_actions_mise::RUST_VERSION));
    assert!(rust.to_string().contains("RUSTUP_TOOLCHAIN"));
    Ok(())
}

#[test]
fn verdict_setup_does_not_request_rust_homes() -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::{CheckoutHistory, StepKind};
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    use velnor_actions_workflow_renderer::native_job_builders::final_job;

    for version in ["1.98.0", "1.99.1"] {
        let catalog = ToolCatalog::pinned().with_project_tool(PinnedTool::Rust, version)?;
        let job = final_job(
            "ubuntu-26.04",
            &[],
            None,
            &catalog,
            CheckoutHistory::ShallowOnPush,
        )
        .map_err(|error| format!("{error:?}"))?;
        for step in &job.steps {
            if let StepKind::Shell { env, .. } = &step.kind {
                for key in ["RUSTUP_TOOLCHAIN", "RUSTUP_HOME", "CARGO_HOME"] {
                    assert!(!env.contains_key(key), "verdict setup retained {key}");
                }
            }
        }
    }
    Ok(())
}
