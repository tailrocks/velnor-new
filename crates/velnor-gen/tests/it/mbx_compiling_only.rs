// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::mbx_default::{NO_LAYOUT, OPTION, PIN, WRAPPER, pinned, with_mise};
use crate::pipeline::workflow;
use crate::yaml_tree::document;

const SHARED: &[u8] = b"shape = 'groups'\nmbx_shared_store = true\n";
const SHARED_STORE: &str = "velnor-ci mbx-store";
const SHARED_SEED: &str = "velnor-ci mbx-seed";
const OUT_DIRS: &str = "Keep mbx build-script outputs in target";

struct Job {
    id: String,
    shared_store: bool,
    compiles: bool,
}

fn compiles(run: &str) -> bool {
    run.contains("cargo-clippy")
        || run.contains("nextest archive")
        || (run.contains("nextest run ") && !run.contains("--archive-file"))
        || run.contains("cargo build")
        || run.contains("cargo doc")
}

fn jobs(version: &str) -> Result<Vec<Job>, Box<dyn std::error::Error>> {
    let mise = format!("{NO_LAYOUT}cargo-machete = \"0.9.1\"\n")
        .replace("rust = \"1.99.1\"\n", &format!("{OPTION}{PIN}"))
        + WRAPPER;
    let repo = with_mise(pinned(Some(SHARED), version)?, &mise)?;
    let yaml = document(&workflow(&repo)?)?;
    let mut jobs = Vec::new();
    for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
        let steps = job["steps"].as_array().ok_or("no steps")?;
        jobs.push(Job {
            id: id.clone(),
            shared_store: steps.iter().any(|step| {
                matches!(
                    step["name"].as_str(),
                    Some(SHARED_STORE | SHARED_SEED | OUT_DIRS)
                )
            }),
            compiles: steps
                .iter()
                .filter_map(|step| step["run"].as_str())
                .any(compiles),
        });
    }
    Ok(jobs)
}

#[test]
fn a_job_that_compiles_nothing_gets_no_shared_store_step_for_a_helper_that_confines_them()
-> TestResult {
    let jobs = jobs("0.1.111")?;
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
    let jobs = jobs("0.1.111")?;
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
fn an_older_helper_still_renders_the_store_in_the_policy_job() -> TestResult {
    let jobs = jobs("0.1.110")?;
    let policy = jobs
        .iter()
        .find(|job| job.id == "policy")
        .ok_or("no policy job")?;
    assert!(policy.shared_store);
    Ok(())
}
