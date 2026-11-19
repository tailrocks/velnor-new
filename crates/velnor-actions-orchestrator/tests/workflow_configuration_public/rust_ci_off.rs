// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{finalized_jobs, plan_internal, prepare, render_staged_tree};

use super::non_inputs_checkout_support::rust_repo;
use super::one_job_per_crate::mbx_repo_for;
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
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(text.as_bytes())
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[(byte >> 4) as usize]),
                char::from(HEX[(byte & 15) as usize]),
            ]
        })
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
    for id in ["cargo-deny", "ledger-cli"] {
        assert!(!ids.iter().any(|have| have == id), "{id} left in ci.yml");
    }
    let parsed: serde_json::Value = serde_saphyr::from_str(&off[CI])?;
    let needs = serde_json::to_string(&parsed["jobs"]["required"])?;
    assert!(
        !needs.contains("rust-"),
        "Required still needs Rust: {needs}"
    );
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

fn ci_false_rust_repo(policy: WorkflowPolicy) -> Result<TempDir, Box<dyn Error>> {
    let repo = rust_repo(policy, None)?;
    let path = repo.path().join(".velnor/config.toml");
    let config = fs::read_to_string(&path)?;
    fs::write(
        &path,
        config.replacen("[stacks.rust]\n", "[stacks.rust]\nci = false\n", 1),
    )?;
    Ok(repo)
}

#[test]
fn ci_false_with_declared_suites_still_builds_and_drops_suite_jobs() -> Result<(), Box<dyn Error>> {
    {
        let policy = WorkflowPolicy::ConsumerV1;
        let repo = ci_false_rust_repo(policy)?;
        let jobs = finalized_jobs(&prepare(repo.path())?)?;
        let ids: Vec<&String> = jobs.keys().collect();
        assert!(
            !ids.iter()
                .any(|id| id.starts_with("suite-") || id.starts_with("rust-")),
            "{policy:?}: {ids:?}"
        );
        assert!(jobs.contains_key("required"), "{policy:?}");
    }
    Ok(())
}

#[test]
fn ci_false_keeps_cargo_machete_in_the_repository_workflow() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_for(false, WorkflowPolicy::VelnorRepositoryV1)?;
    let path = repo.path().join(".velnor/config.toml");
    let mut config = fs::read_to_string(&path)?;
    config.push_str("\n[stacks.rust]\nci = false\n");
    fs::write(&path, config)?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    assert!(jobs.contains_key("cargo-machete"), "{:?}", jobs.keys());
    let required = serde_json::to_string(&jobs["required"])?;
    assert!(required.contains("cargo-machete"), "{required}");
    Ok(())
}

#[test]
fn ci_false_plan_lists_no_rust_jobs() -> Result<(), Box<dyn Error>> {
    let mut seen = Vec::new();
    for ci in [true, false] {
        let repo = mbx_repo_for(false, WorkflowPolicy::ConsumerV1)?;
        if !ci {
            let path = repo.path().join(".velnor/config.toml");
            let mut config = fs::read_to_string(&path)?;
            config.push_str("\n[stacks.rust]\nci = false\n");
            fs::write(&path, config)?;
        }
        let root = repo.path();
        let head = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(root)
                .output()?
                .stdout,
        )?;
        let request = serde_json::json!({
            "schema": 1, "op": "plan-v1", "run_key": "r260001-a1", "base": null,
            "head": head.trim(), "event": "local", "root": root.to_string_lossy(),
        });
        let response: serde_json::Value =
            serde_json::from_str(&plan_internal(&request.to_string())?)?;
        let ids: Vec<String> = response["plan"]["jobs"]
            .as_array()
            .ok_or("plan jobs")?
            .iter()
            .filter_map(|job| job["job_id"].as_str().map(str::to_owned))
            .collect();
        let rust = ids
            .iter()
            .any(|id| id.starts_with("rust-") || id.starts_with("suite-"));
        seen.push((ci, rust, ids));
    }
    assert!(
        seen[0].1,
        "control: default plan must carry Rust jobs: {:?}",
        seen[0].2
    );
    assert!(
        !seen[1].1,
        "ci = false plan carries Rust jobs: {:?}",
        seen[1].2
    );
    Ok(())
}

fn ci_false_velnor_repo() -> Result<TempDir, Box<dyn Error>> {
    let repo = mbx_repo_for(true, WorkflowPolicy::VelnorRepositoryV1)?;
    let path = repo.path().join(".velnor/config.toml");
    let mut config = fs::read_to_string(&path)?;
    config.push_str("\n[stacks.rust]\nci = false\n");
    fs::write(&path, config)?;
    Ok(repo)
}

#[test]
fn ci_false_state_seal_plan_emits_no_unread_plan_attempt() -> Result<(), Box<dyn Error>> {
    let repo = ci_false_velnor_repo()?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let seal = tree
        .files
        .iter()
        .find(|file| file.path == STATE_SEAL)
        .ok_or("state-seal.yml missing")?;
    let parsed: serde_json::Value = serde_saphyr::from_str(&seal.bytes)?;
    let emitted = parsed["jobs"]["plan"]["outputs"]
        .as_object()
        .is_some_and(|outputs| outputs.contains_key("plan_attempt"));
    let read = seal.bytes.contains("needs.plan.outputs.plan_attempt");
    assert_eq!(emitted, read, "plan_attempt emitted={emitted} read={read}");
    Ok(())
}

#[test]
fn ci_false_keeps_ledger_cli_fetch_roots_for_the_repository_workflow() -> Result<(), Box<dyn Error>>
{
    let repo = ci_false_velnor_repo()?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    assert!(jobs.contains_key("ledger-cli"), "{:?}", jobs.keys());
    Ok(())
}
