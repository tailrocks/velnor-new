// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::workflow_configuration_fixture::{git, make_repo};

const CI: &str = ".github/workflows/ci.yml";
const SOURCE_QUALIFICATION: &str = ".github/workflows/source-qualification.yml";

const CONFIG: &str = "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"generator_release\"]\n\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n\n[stacks.rust]\n[stacks.rust.release]\nenabled = true\npackages = [\"velnor-actions-cli\"]\n";

fn repo() -> Result<TempDir, Box<dyn Error>> {
    let repo = make_repo(CONFIG)?;
    let root = repo.path();
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/demo.git",
        ],
        root,
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    git(&["add", "."], root)?;
    Ok(repo)
}

fn render() -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let repo = repo()?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(tree
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect())
}

fn job_ids(workflow: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let parsed: serde_json::Value = serde_saphyr::from_str(workflow)?;
    Ok(parsed["jobs"]
        .as_object()
        .ok_or("jobs")?
        .keys()
        .cloned()
        .collect())
}

#[test]
fn ci_workflow_carries_no_rust_job_and_release_workflows_stay() -> Result<(), Box<dyn Error>> {
    let rendered = render()?;
    let ids = job_ids(&rendered[CI])?;
    assert!(
        !ids.iter().any(|id| id.starts_with("rust-")),
        "Rust jobs in ci.yml: {ids:?}"
    );
    for id in ["cargo-deny", "cargo-machete", "ledger-cli"] {
        assert!(!ids.iter().any(|have| have == id), "{id} in ci.yml");
    }
    let parsed: serde_json::Value = serde_saphyr::from_str(&rendered[CI])?;
    let needs = serde_json::to_string(&parsed["jobs"]["required"])?;
    assert!(!needs.contains("rust-"), "Required needs Rust: {needs}");
    assert!(ids.iter().any(|id| id == "plan"));
    assert!(ids.iter().any(|id| id == "required"));
    assert!(
        !rendered.contains_key(".github/workflows/state-seal.yml"),
        "state seal workflow rendered"
    );
    assert!(
        rendered.keys().any(|path| path.contains("release")),
        "fixture emitted no release workflow"
    );
    Ok(())
}

#[test]
fn source_qualification_runs_for_every_pull_request() -> Result<(), Box<dyn Error>> {
    let rendered = render()?;
    let workflow = rendered
        .get(SOURCE_QUALIFICATION)
        .ok_or("source qualification workflow missing")?;
    assert!(workflow.contains("CLI_ROUTES: '[]'") || workflow.contains("CLI_ROUTES: \"[]\""));
    Ok(())
}
