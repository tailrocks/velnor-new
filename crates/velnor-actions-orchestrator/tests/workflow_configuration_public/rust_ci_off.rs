// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::workflow_configuration_fixture::{git, make_repo};

const CI: &str = ".github/workflows/ci.yml";
const SOURCE_QUALIFICATION: &str = ".github/workflows/source-qualification.yml";
const STATE_SEAL: &str = ".github/workflows/state-seal.yml";

fn config(rust_header: &str) -> String {
    format!(
        "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"generator_release\"]\n\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n\n[stacks.rust]\n{rust_header}[stacks.rust.release]\nenabled = true\npackages = [\"velnor-actions-cli\"]\n"
    )
}

fn repo(rust_header: &str) -> Result<TempDir, Box<dyn Error>> {
    let repo = make_repo(&config(rust_header))?;
    let root = repo.path();
    git(
        &["remote", "add", "origin", "https://github.com/example/demo.git"],
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

fn render(rust_header: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let repo = repo(rust_header)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(tree
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect())
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
fn default_output_is_byte_identical_with_explicit_ci_true() -> Result<(), Box<dyn Error>> {
    let unset = render("")?;
    let explicit = render("ci = true\n")?;
    assert_eq!(unset, explicit);
    assert!(
        job_ids(&unset[CI])?
            .iter()
            .any(|id| id.starts_with("rust-")),
        "default ci.yml must still carry Rust jobs"
    );
    Ok(())
}

#[test]
fn ci_false_drops_every_rust_job_and_keeps_everything_else_byte_identical()
-> Result<(), Box<dyn Error>> {
    let on = render("")?;
    let off = render("ci = false\n")?;
    let ids = job_ids(&off[CI])?;
    assert!(
        !ids.iter().any(|id| id.starts_with("rust-")),
        "Rust jobs left in ci.yml: {ids:?}"
    );
    for id in ["cargo-deny", "cargo-machete", "ledger-cli"] {
        assert!(!ids.iter().any(|have| have == id), "{id} left in ci.yml");
    }
    let parsed: serde_json::Value = serde_saphyr::from_str(&off[CI])?;
    let needs = serde_json::to_string(&parsed["jobs"]["required"])?;
    assert!(!needs.contains("rust-"), "Required still needs Rust: {needs}");
    assert!(ids.iter().any(|id| id == "plan"));
    assert!(ids.iter().any(|id| id == "required"));

    let ci_only: Vec<&String> = on.keys().filter(|path| !off.contains_key(*path)).collect();
    assert_eq!(
        ci_only,
        [
            ".github/actions/declared-task-0/action.yml",
            ".github/velnor/task-execution-manifest-v1.json"
        ],
        "only the Rust CI task files may disappear"
    );
    assert!(off.keys().all(|path| on.contains_key(path)));
    let mut release_files = 0;
    for (path, bytes) in &off {
        if [CI, SOURCE_QUALIFICATION, STATE_SEAL].contains(&path.as_str()) {
            continue;
        }
        assert_eq!(
            sha256(bytes),
            sha256(&on[path]),
            "{path} changed with ci = false"
        );
        if path.contains("release") {
            release_files += 1;
        }
    }
    assert!(release_files > 0, "fixture emitted no release workflow");
    Ok(())
}

#[test]
fn ci_false_keeps_source_qualification_and_runs_it_for_every_pull_request()
-> Result<(), Box<dyn Error>> {
    let off = render("ci = false\n")?;
    let workflow = off
        .get(SOURCE_QUALIFICATION)
        .ok_or("source qualification workflow missing")?;
    assert!(workflow.contains("CLI_ROUTES: '[]'") || workflow.contains("CLI_ROUTES: \"[]\""));
    Ok(())
}
