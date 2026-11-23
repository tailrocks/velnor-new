// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::golden::{load, sample_dirs, samples};
use crate::mbx_default::{NO_LAYOUT, OPTION, PIN, WRAPPER, pinned, with_mise};
use crate::pipeline::workflow;
use crate::yaml_tree::document;
use serde_json::Value;
use velnor_gen::expected_files;
use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};

const SHARED: &[u8] = b"shape = 'groups'\nmbx_shared_store = true\n";
const STORE: &str = "velnor-ci mbx-store";
const SEED: &str = "velnor-ci mbx-seed";
const PRIVATE: &str = "Private mbx store";
const INSTALL: &str = "Install tools";
const OUT_DIRS: &str = "Keep mbx build-script outputs in target";
const COMPILING_JOBS: &[&str] = &["rust-lint", "rust-test"];
const IDLE_JOBS: &[&str] = &["policy", "rust-plan", "rust-test-default"];

fn repo(version: &str, library: bool) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mise = format!("{NO_LAYOUT}cargo-machete = \"0.9.1\"\n")
        .replace("rust = \"1.99.1\"\n", &format!("{OPTION}{PIN}"))
        + WRAPPER;
    let mut repo = with_mise(pinned(Some(SHARED), version)?, &mise)?;
    if library {
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
    }
    Ok(repo)
}

fn rendered_jobs(version: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let yaml = document(&workflow(&repo(version, false)?)?)?;
    Ok(yaml["jobs"].clone())
}

fn step_names(jobs: &Value, id: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let job = jobs.get(id).ok_or_else(|| format!("no {id} job"))?;
    Ok(job["steps"]
        .as_array()
        .ok_or_else(|| format!("{id} has no steps"))?
        .iter()
        .filter_map(|step| step["name"].as_str().map(str::to_owned))
        .collect())
}

#[test]
fn each_compiling_job_renders_the_shared_store_seed_and_out_dirs_steps() -> TestResult {
    let jobs = rendered_jobs("0.1.112")?;
    for id in COMPILING_JOBS {
        let names = step_names(&jobs, id)?;
        for step in [STORE, OUT_DIRS, SEED] {
            assert!(names.iter().any(|name| name == step), "{id} lacks {step:?}");
        }
    }
    Ok(())
}

#[test]
fn an_idle_job_and_the_archive_test_job_render_no_shared_store_steps() -> TestResult {
    let jobs = rendered_jobs("0.1.112")?;
    for id in IDLE_JOBS {
        let names = step_names(&jobs, id)?;
        for step in [STORE, OUT_DIRS, SEED] {
            assert!(
                !names.iter().any(|name| name == step),
                "{id} renders {step:?}"
            );
        }
    }
    let archive = jobs["rust-test-default"]["steps"]
        .as_array()
        .ok_or("rust-test-default has no steps")?;
    assert!(
        archive.iter().any(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("nextest run --archive-file"))
        }),
        "rust-test-default no longer runs the archive"
    );
    Ok(())
}

#[test]
fn a_job_that_runs_cargo_check_renders_the_shared_store_seed_and_out_dirs_steps() -> TestResult {
    let yaml = document(&workflow(&repo("0.1.112", true)?)?)?;
    let jobs = yaml["jobs"].clone();
    let mut checking = Vec::new();
    for (id, job) in jobs.as_object().ok_or("no jobs")? {
        let runs_check = job["steps"]
            .as_array()
            .ok_or("no steps")?
            .iter()
            .any(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("cargo check"))
            });
        if runs_check {
            checking.push(id.clone());
        }
    }
    assert!(!checking.is_empty(), "no job runs cargo check");
    for id in &checking {
        let names = step_names(&jobs, id)?;
        for step in [STORE, OUT_DIRS, SEED] {
            assert!(names.iter().any(|name| name == step), "{id} lacks {step:?}");
        }
    }
    Ok(())
}

#[test]
fn an_older_helper_still_renders_the_store_in_the_policy_job() -> TestResult {
    for version in ["0.1.110", "0.1.111"] {
        let names = step_names(&rendered_jobs(version)?, "policy")?;
        assert!(names.iter().any(|name| name == STORE), "{version}");
    }
    Ok(())
}

fn job_steps<'a>(jobs: &'a Value, id: &str) -> Result<&'a Vec<Value>, Box<dyn std::error::Error>> {
    jobs.get(id)
        .and_then(|job| job["steps"].as_array())
        .ok_or_else(|| format!("no steps for {id}").into())
}

fn sets_cache_dir(step: &Value) -> bool {
    step["run"]
        .as_str()
        .is_some_and(|run| run.contains("MBX_CACHE_DIR"))
        || ["velnor-ci mbx-store", "Set up the mbx cache"]
            .contains(&step["name"].as_str().unwrap_or_default())
}

#[test]
fn the_archive_job_keeps_a_private_mbx_store_and_renders_no_shared_store_steps() -> TestResult {
    let jobs = rendered_jobs("0.1.112")?;
    let steps = job_steps(&jobs, "rust-test-default")?;
    let private = steps
        .iter()
        .find(|step| step["name"] == PRIVATE)
        .ok_or("the archive job has no private mbx store step")?;
    assert_eq!(
        private["run"].as_str(),
        Some("echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx\" >> \"$GITHUB_ENV\"")
    );
    let names = step_names(&jobs, "rust-test-default")?;
    for step in [STORE, OUT_DIRS, SEED] {
        assert!(!names.iter().any(|name| name == step), "renders {step:?}");
    }
    Ok(())
}

#[test]
fn a_compiling_job_renders_the_shared_store_steps_and_sets_the_cache_dir() -> TestResult {
    let jobs = rendered_jobs("0.1.112")?;
    for id in COMPILING_JOBS {
        let names = step_names(&jobs, id)?;
        for step in [STORE, OUT_DIRS, SEED] {
            assert!(names.iter().any(|name| name == step), "{id} lacks {step:?}");
        }
        assert!(job_steps(&jobs, id)?.iter().any(sets_cache_dir), "{id}");
    }
    Ok(())
}

#[test]
fn every_sample_job_that_installs_tools_sets_the_mbx_cache_dir_at_0_1_112() -> TestResult {
    let mut routed = 0;
    for dir in sample_dirs(&samples())? {
        let mut repo = load(&dir)?;
        let lock = RepoPath::new(".velnor/rust-ci.lock")?;
        let Some(bytes) = repo.files.remove(&lock) else {
            continue;
        };
        let text = String::from_utf8(bytes)?;
        let (first, rest) = text.split_once('\n').ok_or("short helper lock")?;
        assert!(first.starts_with("version = "), "{first}");
        let relocked = format!("version = \"0.1.112\"\n{rest}");
        repo.files.insert(lock, relocked.into_bytes());
        for (path, text) in expected_files(&repo)? {
            if !path.as_str().starts_with(".github/workflows/") {
                continue;
            }
            let jobs = document(&text)?["jobs"].clone();
            let Some(jobs) = jobs.as_object() else {
                continue;
            };
            let providers = |job: &Value| {
                job["steps"]
                    .as_array()
                    .is_some_and(|steps| steps.iter().any(sets_cache_dir))
            };
            if !jobs.values().any(providers) {
                continue;
            }
            routed += 1;
            for (id, job) in jobs {
                let installs = job["steps"]
                    .as_array()
                    .is_some_and(|steps| steps.iter().any(|step| step["name"] == INSTALL));
                assert!(
                    !installs || providers(job),
                    "{}: {path:?} job {id} installs tools without MBX_CACHE_DIR",
                    dir.display()
                );
            }
        }
    }
    assert!(routed > 0, "no sample routes cargo through mbx");
    Ok(())
}
