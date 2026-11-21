// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::mbx_default::{NO_LAYOUT, OPTION, PIN, WRAPPER, pinned, with_mise};
use crate::pipeline::workflow;
use crate::yaml_tree::document;
use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};

const SHARED: &[u8] = b"shape = 'groups'\nmbx_shared_store = true\n";
const SHARED_STORE: &str = "velnor-ci mbx-store";
const SHARED_SEED: &str = "velnor-ci mbx-seed";
const OUT_DIRS: &str = "Keep mbx build-script outputs in target";

const BUILDING: &[&str] = &[
    "cargo check",
    "cargo build",
    "cargo test",
    "cargo doc",
    "cargo-clippy",
    "nextest archive",
    "nextest run",
    "velnor-ci lane-guard",
];

struct Job {
    id: String,
    shared_store: bool,
    compiles: bool,
    checks: bool,
}

fn compiles(run: &str) -> bool {
    let reads_archive = run.contains("nextest run") && run.contains("--archive-file");
    !reads_archive && BUILDING.iter().any(|command| run.contains(command))
}

fn render(repo: &RepoView) -> Result<Vec<Job>, Box<dyn std::error::Error>> {
    let yaml = document(&workflow(repo)?)?;
    let mut jobs = Vec::new();
    for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
        let steps = job["steps"].as_array().ok_or("no steps")?;
        let runs: Vec<&str> = steps
            .iter()
            .filter_map(|step| step["run"].as_str())
            .collect();
        jobs.push(Job {
            id: id.clone(),
            shared_store: steps.iter().any(|step| {
                matches!(
                    step["name"].as_str(),
                    Some(SHARED_STORE | SHARED_SEED | OUT_DIRS)
                )
            }),
            compiles: runs.iter().any(|run| compiles(run)),
            checks: runs.iter().any(|run| run.contains("cargo check")),
        });
    }
    Ok(jobs)
}

fn repo(version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mise = format!("{NO_LAYOUT}cargo-machete = \"0.9.1\"\n")
        .replace("rust = \"1.99.1\"\n", &format!("{OPTION}{PIN}"))
        + WRAPPER;
    with_mise(pinned(Some(SHARED), version)?, &mise)
}

fn jobs(version: &str) -> Result<Vec<Job>, Box<dyn std::error::Error>> {
    render(&repo(version)?)
}

fn jobs_with_a_library(version: &str) -> Result<Vec<Job>, Box<dyn std::error::Error>> {
    let mut repo = repo(version)?;
    let manifest = repo
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no Cargo.toml metadata")?;
    manifest.members[0].targets.push(Target {
        name: TargetName::new("lib")?,
        kinds: vec![TargetKind::Lib],
        tests: false,
        doctest: false,
        source: None,
        requires_features: false,
    });
    render(&repo)
}

#[test]
fn a_job_that_compiles_nothing_gets_no_shared_store_step_for_a_helper_that_confines_them()
-> TestResult {
    let jobs = jobs("0.1.112")?;
    let idle: Vec<_> = jobs.iter().filter(|job| !job.compiles).collect();
    assert!(
        idle.iter().any(|job| job.id == "policy"),
        "no idle policy job"
    );
    for job in idle {
        assert!(!job.shared_store, "{} compiles nothing", job.id);
    }
    Ok(())
}

#[test]
fn every_compiling_job_keeps_the_shared_store_and_its_seed() -> TestResult {
    let jobs = jobs("0.1.112")?;
    let compiling: Vec<_> = jobs.iter().filter(|job| job.compiles).collect();
    assert!(!compiling.is_empty());
    for job in compiling {
        assert!(
            job.shared_store,
            "{} compiles and needs the shared store",
            job.id
        );
    }
    Ok(())
}

#[test]
fn a_job_that_runs_cargo_check_keeps_the_shared_store_and_its_seed() -> TestResult {
    let jobs = jobs_with_a_library("0.1.112")?;
    let checking: Vec<_> = jobs.iter().filter(|job| job.checks).collect();
    assert!(!checking.is_empty(), "no job runs cargo check");
    for job in checking {
        assert!(
            job.shared_store,
            "{} runs cargo check and lost its shared store",
            job.id
        );
    }
    Ok(())
}

#[test]
fn an_older_helper_still_renders_the_store_in_the_policy_job() -> TestResult {
    for version in ["0.1.110", "0.1.111"] {
        let jobs = jobs(version)?;
        let policy = jobs
            .iter()
            .find(|job| job.id == "policy")
            .ok_or("no policy job")?;
        assert!(policy.shared_store, "{version}");
    }
    Ok(())
}
