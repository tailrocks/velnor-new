//! Exact Cargo-only source-cache archive ownership.

use velnor_actions_orchestrator::GenerationPreparation;

use crate::impl_cache_fixtures::{action_inputs, crate_jobs, job_steps, prep_for, yaml_for};
use crate::impl_common::TestResult;

#[test]
fn cargo_only_uses_same_exact_sources_archive() -> TestResult {
    let (_repo, prep) = prep_for(false)?;
    assert_shared_source_archive(&prep)?;
    let yaml = yaml_for(false)?;
    assert!(!yaml.contains("mr-boxington-action"), "no MBX stacked");
    assert!(
        yaml.contains("Save Cargo sources"),
        "plan seeds one snapshot"
    );
    assert!(
        !yaml.contains("Swatinem/rust-cache"),
        "broad archive removed"
    );
    Ok(())
}

fn assert_shared_source_archive(prep: &GenerationPreparation) -> TestResult {
    let mut jobs = vec!["plan".to_owned()];
    jobs.extend(crate_jobs(prep));
    let mut shared = String::new();
    for job in jobs {
        let steps = job_steps(prep, &job)?;
        let with = action_inputs(steps, "Restore Cargo sources")?;
        let key = with.get("key").cloned().ok_or("key")?;
        assert!(key.starts_with("velnor-v1-sources-"), "{key}");
        if shared.is_empty() {
            shared.clone_from(&key);
        }
        assert_eq!(key, shared, "{job} shares the sources key");
        assert_source_paths(&job, with.get("path").ok_or("source paths")?);
        check_source_writer(&job, steps, &key)?;
    }
    Ok(())
}

fn assert_source_paths(job: &str, path: &str) {
    for owned in [
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/git/db",
    ] {
        assert!(path.contains(owned), "{job} omits {owned}: {path}");
    }
    for excluded in [
        "${{ runner.temp }}/velnor/cargo/bin",
        ".crates.toml",
        ".crates2.json",
    ] {
        assert!(
            !path.contains(excluded),
            "{job} includes {excluded}: {path}"
        );
    }
}

fn check_source_writer(
    job: &str,
    steps: &[velnor_actions_contract::Step],
    key: &str,
) -> TestResult {
    if job == "plan" {
        let save = action_inputs(steps, "Save Cargo sources")?;
        assert_eq!(save.get("key").map(String::as_str), Some(key));
    } else {
        assert!(!steps.iter().any(|step| step.name == "Save Cargo sources"));
    }
    Ok(())
}
