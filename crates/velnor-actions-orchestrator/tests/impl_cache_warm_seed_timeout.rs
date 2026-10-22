//! Dependency-seed jobs use their own workflow timeout.

use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::Job;
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::{TestResult, lock_for, make_workspace};

#[test]
fn seed_timeout_is_independent_from_crate_timeout() -> TestResult {
    let repo = make_workspace(&lock_for(&["a", "b"]), true)?;
    let nested_member = repo.path().join("nested/member");
    fs::create_dir_all(nested_member.join("src"))?;
    fs::write(
        repo.path().join("nested/Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )?;
    fs::write(
        nested_member.join("Cargo.toml"),
        "[package]\nname = \"nested-member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(nested_member.join("src/lib.rs"), "pub fn f() {}\n")?;
    fs::write(
        repo.path().join("nested/Cargo.lock"),
        lock_for(&["nested-member"]),
    )?;
    let config_path = repo.path().join(".velnor/config.toml");
    fs::write(
        config_path,
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\ncrate_timeout_minutes = 10\n",
    )?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    assert_timeout(&jobs, "dependency-seed", 30)?;
    assert_timeout(&jobs, "dependency-seed-nested", 30)?;
    assert_crate_timeouts(&jobs, 10)?;

    fs::write(
        repo.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\ncrate_timeout_minutes = 10\ndependency_seed_timeout_minutes = 45\n",
    )?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    assert_timeout(&jobs, "dependency-seed", 45)?;
    assert_timeout(&jobs, "dependency-seed-nested", 45)?;
    assert_crate_timeouts(&jobs, 10)?;
    Ok(())
}

fn assert_timeout(jobs: &BTreeMap<String, Job>, id: &str, expected: u16) -> TestResult {
    let job = jobs
        .get(id)
        .ok_or_else(|| std::io::Error::other(format!("missing job {id}")))?;
    assert_eq!(job.timeout_minutes.minutes(), expected, "{id}");
    Ok(())
}

fn assert_crate_timeouts(jobs: &BTreeMap<String, Job>, expected: u16) -> TestResult {
    let mut crate_jobs = jobs.iter().filter(|(id, _)| id.starts_with("rust-"));
    let first = crate_jobs
        .next()
        .ok_or_else(|| std::io::Error::other("missing Rust crate jobs"))?;
    for (id, job) in std::iter::once(first).chain(crate_jobs) {
        assert_eq!(job.timeout_minutes.minutes(), expected, "{id}");
    }
    Ok(())
}
