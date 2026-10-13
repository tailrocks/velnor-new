//! Plan/generate parity: plan lists the finalized jobs it renders.
//!
//! Every case prepares a fixture, renders the staged tree `generate`
//! writes, and requires the plan text to name the same job IDs with
//! the same step counts plus the same action pins the YAML embeds.
//! Validators, conditionals, and empty repos are covered per case so a
//! plan/YAML gap fails here instead of shipping stale `plan` output.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use velnor_actions_contract_workflow::Job;
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::impl_common::{
    TestResult, config_with_branch, fixture_manifest_json, git, make_repo, plan_for,
    without_ambient_identity,
};

/// Minimal lockfile making a fixture lockful (fetch/restore emitted).
const FAKE_LOCK: &str = "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n";

/// Velnor-policy fixture with canonical origin plus release manifest.
fn make_velnor_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        root,
    )?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n",
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Job IDs in the staged workflow: 2-space keys under `jobs:`.
fn yaml_job_ids(yaml: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut in_jobs = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if in_jobs {
            if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
                ids.insert(line.trim().trim_end_matches(':').to_owned());
            } else if !line.starts_with(' ') && !line.is_empty() {
                break;
            }
        }
    }
    ids
}

/// Distinct `uses:` refs embedded in the staged workflow.
fn yaml_uses(yaml: &str) -> BTreeSet<String> {
    yaml.lines()
        .filter_map(|line| line.trim().strip_prefix("uses:"))
        .map(|value| {
            let value = value.trim();
            value
                .split_once(" # zizmor:")
                .map_or(value, |(pin, _)| pin)
                .to_owned()
        })
        .filter(|uses| !uses.starts_with("./.github/actions/"))
        .collect()
}

/// `Jobs:` table parsed from plan text: id plus advertised step count.
fn plan_job_lines(plan: &str) -> Result<BTreeMap<String, usize>, Box<dyn std::error::Error>> {
    let mut jobs = BTreeMap::new();
    let mut in_jobs = false;
    for line in plan.lines() {
        if line == "  Jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let Some(entry) = line.strip_prefix("    - ") else {
            break;
        };
        let Some((id, rest)) = entry.split_once(" (") else {
            break;
        };
        let Some(count) = rest.strip_suffix(" steps)") else {
            break;
        };
        let count: usize = count
            .parse()
            .map_err(|err| format!("step count parses for {id}: {err}"))?;
        jobs.insert(id.to_owned(), count);
    }
    Ok(jobs)
}

/// `Action pins:` list parsed from plan text.
fn plan_pins(plan: &str) -> BTreeSet<String> {
    let mut pins = BTreeSet::new();
    let mut in_pins = false;
    for line in plan.lines() {
        if line.starts_with("  Action pins:") {
            in_pins = true;
            continue;
        }
        if !in_pins {
            continue;
        }
        if let Some(pin) = line.strip_prefix("    - ") {
            pins.insert(pin.to_owned());
        } else {
            break;
        }
    }
    pins
}

/// Assert plan text, finalized jobs, and staged YAML agree on jobs/pins.
fn assert_parity(prep: &GenerationPreparation, plan: &str, yaml: &str) -> TestResult {
    let finalized: BTreeMap<String, Job> = finalized_jobs(prep)?;
    let listed = plan_job_lines(plan)?;
    assert_eq!(
        listed.len(),
        finalized.len(),
        "plan lists every finalized job:\n{plan}"
    );
    for (id, job) in &finalized {
        assert_eq!(
            listed.get(id),
            Some(&job.steps.len()),
            "step count parity for {id}:\n{plan}"
        );
    }
    let yaml_ids = yaml_job_ids(yaml);
    let finalized_ids: BTreeSet<String> = finalized.keys().cloned().collect();
    assert_eq!(yaml_ids, finalized_ids, "yaml job parity");
    assert_eq!(plan_pins(plan), yaml_uses(yaml), "action pin parity");
    Ok(())
}

/// Render the staged workflow YAML `generate` would write.
fn workflow_yaml(prep: &GenerationPreparation) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(prep)?;
    tree.get(WORKFLOW_PATH)
        .ok_or_else(|| "missing workflow in staged tree".into())
        .map(str::to_owned)
}

#[test]
fn parity_consumer_lockless() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let plan = plan_for(&prep)?;
    let yaml = workflow_yaml(&prep)?;
    assert_parity(&prep, &plan, &yaml)?;
    assert!(plan.contains("Rust: selected"), "stack line:\n{plan}");
    assert!(
        !plan.contains("Cargo sources"),
        "lockless promises no sources layer:\n{plan}"
    );
    assert!(
        !yaml.contains("Fetch Cargo sources"),
        "lockless emits no fetch"
    );
    Ok(())
}

#[test]
fn parity_consumer_lockful() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("Cargo.lock"), FAKE_LOCK)?;
    let prep = prepare(repo.path())?;
    let plan = plan_for(&prep)?;
    let yaml = workflow_yaml(&prep)?;
    assert_parity(&prep, &plan, &yaml)?;
    assert!(
        plan.contains("Cargo sources"),
        "lockful advertises the layer:\n{plan}"
    );
    assert!(
        yaml.contains("Fetch Cargo sources"),
        "lockful emits the fetch"
    );
    Ok(())
}

#[test]
fn parity_no_work_repo() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::remove_file(repo.path().join("Cargo.toml"))?;
    fs::remove_dir_all(repo.path().join("src"))?;
    let prep = prepare(repo.path())?;
    let plan = plan_for(&prep)?;
    let yaml = workflow_yaml(&prep)?;
    assert_parity(&prep, &plan, &yaml)?;
    assert!(
        plan.contains("Rust: none detected"),
        "empty repo is not selected:\n{plan}"
    );
    assert!(
        plan.contains("no matrix entries (no-work workflow)"),
        "no-work line:\n{plan}"
    );
    Ok(())
}

/// Pure-tofu fixture: two configured roots, no Cargo.
fn make_pure_tofu_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\"]\n",
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    for repo_root in ["stacks/a", "stacks/b"] {
        fs::create_dir_all(root.join(repo_root))?;
        fs::write(root.join(repo_root).join("main.tf"), "variable \"x\" {}\n")?;
    }
    Ok(dir)
}

#[test]
fn parity_pure_tofu() -> TestResult {
    let repo = make_pure_tofu_repo()?;
    let prep = prepare(repo.path())?;
    let plan = plan_for(&prep)?;
    let yaml = workflow_yaml(&prep)?;
    assert_parity(&prep, &plan, &yaml)?;
    for id in ["tofu-stacks-a", "tofu-stacks-b"] {
        assert!(
            plan.contains(&format!("- {id} (")),
            "plan lists {id}:\n{plan}"
        );
        assert!(yaml.contains(&format!("\n  {id}:")), "yaml has {id}");
        assert!(
            !plan.contains("Rust crate job"),
            "no rust label on a pure-tofu plan:\n{plan}"
        );
    }
    Ok(())
}

#[test]
fn parity_velnor_policy_lists_validators() -> TestResult {
    without_ambient_identity("parity_velnor_policy_lists_validators", || {
        let repo = make_velnor_repo()?;
        let prep = prepare(repo.path())?;
        let plan = plan_for(&prep)?;
        let yaml = workflow_yaml(&prep)?;
        assert_parity(&prep, &plan, &yaml)?;
        for id in ["alint", "cargo-deny", "cargo-machete", "zizmor"] {
            assert!(
                plan.contains(&format!("- {id} (")),
                "plan lists {id}:\n{plan}"
            );
            assert!(yaml.contains(&format!("\n  {id}:")), "yaml has {id}");
        }
        assert!(plan.contains("asamarts/alint@"), "plan pins alint:\n{plan}");
        // Merged support jobs never inflate the crate count.
        assert!(
            plan.contains("Parallel: 1 independent crate job; no matrix fan-out"),
            "parallel line:\n{plan}"
        );
        Ok(())
    })
}
