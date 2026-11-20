// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, ResultTest, fixture, read};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

const MBX_LINE: &str = "mbx[cache]: object cache: 0 hits, 0 misses, 12 not looked up; 0 B downloaded, 0 B uploaded, 0 B stored locally";

fn cache_plan() -> serde_json::Value {
    json!([{"family":"tools","root":null,"restore_step":"tools","save_step":"save_tools"}])
}

const TOOLS_HIT: &str = r#"{"tools":{"outcome":"success","outputs":{"cache-primary-key":"tools-key","cache-matched-key":"tools-key","cache-hit":"true"}}}"#;

fn primed_store(repo: &Repo) -> ResultTest {
    let object = repo.root().join("store/actions/cas/object");
    fs::create_dir_all(object.parent().ok_or("parent")?)?;
    fs::write(&object, "entry")?;
    fs::File::options()
        .write(true)
        .open(&object)?
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000))?;
    Ok(())
}

fn run_cargo(repo: &Repo) -> ResultTest {
    let cargo = repo.root().join("cargo-fixture");
    std::os::unix::fs::symlink("/bin/sh", &cargo)?;
    let script = format!("printf '%s\\n' '{MBX_LINE}' >&2");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", cargo.to_str().ok_or("path")?, "-c", &script])
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

fn with_store(
    repo: &Repo,
    store: bool,
) -> Result<velnor_ci::report::LaneMetrics, Box<dyn std::error::Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_REPORT_CACHES", cache_plan().to_string())
        .env("VELNOR_REPORT_STEPS", TOOLS_HIT)
        .env_remove("GITHUB_TOKEN")
        .env_remove("MBX_CACHE_DIR");
    if store {
        command.env("MBX_CACHE_DIR", repo.root().join("store"));
    }
    let output = command.output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(read(repo))
}

#[test]
fn report_records_mbx_sessions_a_primed_store_and_toolchain_installs() -> ResultTest {
    let repo = fixture("report-waste-local")?;
    repo.git(&["add", "."]);
    run_cargo(&repo)?;
    primed_store(&repo)?;
    fs::write(
        repo.root().join("velnor-toolchain-installs.txt"),
        "component rust-src\n",
    )?;
    let metrics = with_store(&repo, true)?;
    assert!(metrics.mbx.store_primed, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions.len(), 1);
    assert_eq!(metrics.mbx.sessions[0].not_looked_up, 12);
    assert_eq!(metrics.toolchain_installs, ["component rust-src"]);
    assert!(
        metrics.violations.iter().any(|line| line
            .contains("toolchain installed on an exact tools cache hit: component rust-src")),
        "{:?}",
        metrics.violations
    );
    let unprimed = with_store(&repo, false)?;
    assert!(!unprimed.mbx.store_primed);
    assert!(
        !unprimed
            .violations
            .iter()
            .any(|line| line.contains("MBX object cache")),
        "{:?}",
        unprimed.violations
    );
    Ok(())
}

fn sources_entry(created_at: &str, reference: &str, version: &str) -> serde_json::Value {
    json!({"key":"velnor-src-key","ref":reference,"created_at":created_at,"version":version,"size_in_bytes":9})
}

fn check_sources_miss(
    name: &str,
    entries: Vec<serde_json::Value>,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let repo = Repo::new(name);
    let mut lane = crate::report::lane();
    lane["job_name"] = json!("Rust build");
    lane["caches"] = json!([{"family":"sources","root":null,"restore_step":"sources","save_step":"save_sources","requested_key":"velnor-src-key","matched_key":null,"hit":"miss","bytes":null,"usable":false,"usable_same_lock":false,"save":"not_attempted"}]);
    lane["invocations"][0]["compilations"] = json!([]);
    fs::write(repo.root().join("ci-metrics.json"), lane.to_string())?;
    let mut listed = entries;
    listed.push(sources_entry(
        "2026-11-20T09:00:00.000Z",
        "refs/heads/elsewhere",
        "v1",
    ));
    let api = crate::io::serve(BTreeMap::from([
        ("/repos/owner/repo/actions/runs/1/attempts/1/jobs?per_page=100&page=1".to_owned(), json!({"total_count":1,"jobs":[{"id":1,"name":"Rust build","created_at":"2026-11-20T10:00:00Z","started_at":"2026-11-20T10:00:00Z"}]}).to_string()),
        ("/repos/owner/repo/actions/caches?key=velnor-src-key&per_page=100".to_owned(), json!({"total_count":listed.len(),"actions_caches":listed}).to_string()),
    ]));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root())
        .env("GITHUB_REF", "refs/pull/1/merge")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_TOKEN", "fixture")
        .env("GITHUB_API_URL", api)
        .env("VELNOR_REPORT_NEEDS", "{}")
        .env("GITHUB_STEP_SUMMARY", repo.root().join("summary"))
        .output()?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

#[test]
fn check_fails_a_sources_miss_whose_entry_predates_the_job_and_passes_a_newer_one() -> ResultTest {
    let before = sources_entry("2026-11-20T09:30:00.000Z", "refs/heads/main", "v1");
    let own_save = sources_entry("2026-11-20T10:05:00.000Z", "refs/heads/main", "v1");
    let (passed, stderr) =
        check_sources_miss("report-sources-miss-old", vec![before.clone(), own_save])?;
    assert!(!passed, "{stderr}");
    assert!(
        stderr.contains("sources cache missed velnor-src-key"),
        "{stderr}"
    );
    let (passed, stderr) = check_sources_miss(
        "report-sources-miss-new",
        vec![sources_entry(
            "2026-11-20T10:05:00.000Z",
            "refs/heads/main",
            "v1",
        )],
    )?;
    assert!(passed, "{stderr}");
    Ok(())
}

#[test]
fn check_passes_a_sources_miss_whose_earlier_entry_has_another_cache_version() -> ResultTest {
    let (passed, stderr) = check_sources_miss(
        "report-sources-miss-other-version",
        vec![
            sources_entry("2026-11-20T09:30:00.000Z", "refs/heads/main", "v1"),
            sources_entry("2026-11-20T10:05:00.000Z", "refs/heads/main", "v2"),
        ],
    )?;
    assert!(passed, "{stderr}");
    Ok(())
}

#[test]
fn check_passes_a_sources_miss_when_the_job_version_cannot_be_established() -> ResultTest {
    let (passed, stderr) = check_sources_miss(
        "report-sources-miss-unknown-version",
        vec![sources_entry(
            "2026-11-20T09:30:00.000Z",
            "refs/heads/main",
            "v1",
        )],
    )?;
    assert!(passed, "{stderr}");
    Ok(())
}

#[test]
fn report_records_that_the_checkout_declares_mbx() -> ResultTest {
    let repo = fixture("report-declares-mbx")?;
    repo.git(&["add", "."]);
    run_cargo(&repo)?;
    assert!(!with_store(&repo, false)?.mbx.declared);
    fs::write(
        repo.root().join("mise.toml"),
        "[wrappers.cargo]\ncommand = \"mbx\"\n",
    )?;
    let metrics = with_store(&repo, false)?;
    assert!(metrics.mbx.declared, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions.len(), 1);
    assert!(
        !metrics
            .violations
            .iter()
            .any(|line| line.contains("compiled without mbx")),
        "{:?}",
        metrics.violations
    );
    Ok(())
}
