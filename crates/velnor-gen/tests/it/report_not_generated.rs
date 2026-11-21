// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::{TestResult, package, path, repo};
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::workflow;
use velnor_model::{RepoPath, RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, pin_lock};

const LOCK: &str = "version = 4\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#abc\"\n";

fn caches_of(job: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut caches = Vec::new();
    for report in job["steps"]
        .as_array()
        .ok_or("steps")?
        .iter()
        .filter(|step| step["run"] == "velnor-ci report")
    {
        let text = report["env"]["VELNOR_REPORT_CACHES"]
            .as_str()
            .ok_or("cache descriptors")?;
        caches.extend(serde_json::from_str::<Vec<Value>>(text)?);
    }
    Ok(caches)
}

fn parsed(view: &velnor_model::RepoView) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_saphyr::from_str(&crate::archive::plain_invocations(
        &workflow(view)?,
    ))?)
}

#[test]
fn refused_base_caches_say_why_when_the_helper_reads_the_reason() -> TestResult {
    for (helper, reason) in [("0.1.110", None), ("0.1.111", Some("private dependencies"))] {
        let mut view = crate::base_save::pinned(helper, "")?;
        view.files
            .insert(RepoPath::new("Cargo.lock")?, LOCK.as_bytes().to_vec());
        let parsed = parsed(&view)?;
        let mut checked = 0;
        for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
            for cache in caches_of(job)? {
                if cache["family"] == "tools" {
                    assert!(cache.get("not_generated").is_none(), "{cache}");
                    continue;
                }
                checked += 1;
                assert_eq!(
                    cache.get("not_generated").and_then(|value| value.as_str()),
                    reason,
                    "{helper}: {cache}"
                );
            }
        }
        assert!(checked > 0, "{helper}");
    }
    Ok(())
}

#[test]
fn the_helper_bootstrap_job_never_saves_so_its_caches_are_not_labelled() -> TestResult {
    let helper = package(
        "velnor-ci",
        "ci-helper/Cargo.toml",
        &[],
        &[("velnor-ci", TargetKind::Bin, true)],
    )?;
    let mise = "[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n\n[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
    let mut view = repo(
        vec![("ci-helper/Cargo.toml", vec![helper])],
        &[("mise.toml", mise)],
    )?;
    view.files.insert(
        path(".velnor/rust-ci.lock")?,
        pin_lock("0.1.111", COMMIT_FILE, SUMS, None)?.into_bytes(),
    );
    view.tracked_paths.insert(CONFIG_PATH);
    view.files.insert(
        CONFIG_PATH,
        b"helper = { path = \"ci-helper/\" }\n".to_vec(),
    );
    view.files
        .insert(path("ci-helper/Cargo.lock")?, LOCK.as_bytes().to_vec());
    let parsed = parsed(&view)?;
    let jobs = parsed["jobs"].as_object().ok_or("jobs")?;
    let bootstrap = caches_of(jobs.get("rust-helper").ok_or("no helper job")?)?;
    assert_ne!(bootstrap.len(), 0);
    for cache in &bootstrap {
        assert!(cache.get("not_generated").is_none(), "{cache}");
    }
    let lane = caches_of(jobs.get("rust").ok_or("no lane job")?)?;
    assert!(
        lane.iter()
            .any(|cache| cache["not_generated"] == "private dependencies"),
        "{lane:?}"
    );
    Ok(())
}

#[test]
fn every_save_step_the_report_names_is_rendered_in_its_job() -> TestResult {
    let lock = "version = 4\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#abc\"\n";
    let mut repo = crate::base_save::pinned("0.1.112", "")?;
    repo.files
        .insert(RepoPath::new("Cargo.lock")?, lock.as_bytes().to_vec());
    assert_named_saves_are_rendered(&repo)
}

#[test]
fn a_public_lint_job_names_only_the_saves_it_renders() -> TestResult {
    assert_named_saves_are_rendered(&crate::base_save::pinned(
        "0.1.112",
        "shape = \"lint-test\"\n",
    )?)
}

fn assert_named_saves_are_rendered(repo: &RepoView) -> TestResult {
    let parsed: serde_json::Value =
        serde_saphyr::from_str(&crate::archive::plain_invocations(&workflow(repo)?))?;
    let mut named = 0;
    for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
        let steps = job["steps"].as_array().ok_or("steps")?;
        let ids: Vec<&str> = steps
            .iter()
            .filter_map(|step| step["id"].as_str())
            .collect();
        for report in steps
            .iter()
            .filter(|step| step["run"] == "velnor-ci report")
        {
            let caches: serde_json::Value = serde_json::from_str(
                report["env"]["VELNOR_REPORT_CACHES"]
                    .as_str()
                    .ok_or("cache descriptors")?,
            )?;
            for cache in caches.as_array().ok_or("caches")? {
                if let Some(save) = cache["save_step"].as_str() {
                    named += 1;
                    assert!(
                        ids.contains(&save),
                        "{save} is named but not rendered: {cache}"
                    );
                }
            }
        }
    }
    assert!(named > 0);
    Ok(())
}

#[test]
fn a_released_pin_keeps_a_save_step_on_every_cache_descriptor() -> TestResult {
    let parsed = parsed(&crate::base_save::pinned(
        "0.1.110",
        "shape = \"lint-test\"\n",
    )?)?;
    let mut checked = 0;
    for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
        for cache in caches_of(job)? {
            checked += 1;
            assert!(cache["save_step"].is_string(), "{cache}");
        }
    }
    assert!(checked > 0);
    Ok(())
}
