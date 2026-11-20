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

const REAL_MBX_1_30_LOG: &str = "\
mbx[cache]: object cache: 0 hits, 89 misses, 12 not looked up; 0 B downloaded, 0 B uploaded, 77.5 MiB stored locally
mbx[cache]: 27.42s session; 1.08s estimated compiler time avoided (summed across compilations)
mbx[cache]: 12 compilations had no usable prior inputs or matching prediction for a lookup
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above
mbx[cache]: object cache: 0 hits, 3 misses; 0 B downloaded, 0 B uploaded, 1.1 MiB stored locally
mbx[cache]: 662.5ms session; 0s estimated compiler time avoided (summed across compilations)
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above
mbx[warning]: prediction was not restored: cached action is missing blob af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
";
const REAL_MBX_1_30_DETAIL_ONLY: &str = "\
mbx[cache]: 12 compilations had no usable prior inputs or matching prediction for a lookup
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above
";
const SINGULAR_SUMMARY: &str = "mbx[cache]: object cache: 3 hit, 0 miss; 0 B downloaded";
const ZERO_HIT_SUMMARY: &str = "mbx[cache]: object cache: 0 hits, 5 misses; 0 B downloaded";
const PARTIAL_SUMMARY: &str = "mbx[cache]: object cache: 4 hits, many misses; 0 B downloaded";

fn registry_compiled_unit() -> serde_json::Value {
    json!({
        "package_id":"registry+https://example.invalid/index#serde@1.0.0",
        "manifest_path":"/registry/serde/Cargo.toml",
        "target":{"name":"serde","kind":["lib"],"crate_types":["lib"],"src_path":"/registry/serde/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":["target/debug/deps/libserde-hash.rmeta"],"fresh":false
    })
}

fn lane_with_mbx_log(
    name: &str,
    log: &str,
    declared: bool,
    primed: bool,
) -> Result<velnor_ci::report::LaneMetrics, Box<dyn std::error::Error>> {
    let repo = fixture(name)?;
    if declared {
        repo.write("mise.toml", "[wrappers.cargo]\ncommand = \"mbx\"\n");
    }
    if primed {
        primed_store(&repo)?;
    }
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["argv"] = json!(["cargo", "build", "--workspace"]);
    invocation["started_at"] = json!(2_000_000);
    invocation["compilations"] = json!([]);
    invocation["progress"] = serde_json::Value::Null;
    invocation["log_start"] = json!(0);
    invocation["log_end"] = json!(log.len());
    invocation["artifacts"] = json!([registry_compiled_unit()]);
    fs::write(repo.root().join("velnor-run.log"), log)?;
    fs::write(
        repo.root().join("velnor-invocations.jsonl"),
        format!("{invocation}\n"),
    )?;
    with_store(&repo, primed)
}

fn unparsed_warning(metrics: &velnor_ci::report::LaneMetrics, first: &str) -> bool {
    metrics.verification_errors.iter().any(|line| {
        line == &format!("MBX lookup summary not understood, 1 line(s) not counted; first: {first}")
    })
}

/// An unparsed summary leaves the lane unverifiable, so the final check fails closed.
fn fails_closed(metrics: &velnor_ci::report::LaneMetrics) -> bool {
    velnor_ci::report::check(std::slice::from_ref(metrics)).is_err_and(|error| {
        error
            .to_string()
            .contains("MBX lookup summary not understood")
    })
}

fn waste_gates(metrics: &velnor_ci::report::LaneMetrics) -> Vec<&String> {
    metrics
        .violations
        .iter()
        .filter(|line| line.contains("MBX object cache") || line.contains("compiled without mbx"))
        .collect()
}

#[test]
fn a_singular_mbx_summary_is_unparsed_with_a_warning_and_never_a_silent_zero() -> ResultTest {
    let metrics = lane_with_mbx_log(
        "report-mbx-singular",
        &format!("{SINGULAR_SUMMARY}\n"),
        true,
        false,
    )?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    assert!(
        unparsed_warning(&metrics, SINGULAR_SUMMARY),
        "{:?}",
        metrics.verification_errors
    );
    assert!(fails_closed(&metrics), "{metrics:?}");
    assert!(waste_gates(&metrics).is_empty(), "{:?}", metrics.violations);
    let silent = lane_with_mbx_log("report-mbx-no-summary", "", true, false)?;
    assert_eq!(silent.mbx.unparsed, 0);
    assert!(
        silent
            .violations
            .iter()
            .any(|line| line.contains("compiled without mbx")),
        "{:?}",
        silent.violations
    );
    Ok(())
}

#[test]
fn partial_mbx_evidence_does_not_fire_the_zero_hit_gate() -> ResultTest {
    let whole = lane_with_mbx_log(
        "report-mbx-zero-hit",
        &format!("{ZERO_HIT_SUMMARY}\n"),
        true,
        true,
    )?;
    assert!(
        whole
            .violations
            .iter()
            .any(|line| line.contains("MBX object cache: 0 hits")),
        "{:?}",
        whole.violations
    );
    let partial = lane_with_mbx_log(
        "report-mbx-partial",
        &format!("{ZERO_HIT_SUMMARY}\n{PARTIAL_SUMMARY}\n"),
        true,
        true,
    )?;
    assert_eq!(partial.mbx.sessions.len(), 1, "{partial:?}");
    assert_eq!(partial.mbx.unparsed, 1, "{partial:?}");
    assert!(
        unparsed_warning(&partial, PARTIAL_SUMMARY),
        "{:?}",
        partial.verification_errors
    );
    assert!(fails_closed(&partial), "{partial:?}");
    assert!(waste_gates(&partial).is_empty(), "{:?}", partial.violations);
    Ok(())
}

#[test]
fn real_mbx_1_30_detail_lines_are_not_unparsed_and_both_gates_stay_active() -> ResultTest {
    let zero_hit = lane_with_mbx_log("report-mbx-real-zero-hit", REAL_MBX_1_30_LOG, true, true)?;
    assert_eq!(zero_hit.mbx.sessions.len(), 2, "{zero_hit:?}");
    assert_eq!(zero_hit.mbx.unparsed, 0, "{zero_hit:?}");
    assert!(
        zero_hit
            .violations
            .iter()
            .any(|line| line.contains("MBX object cache: 0 hits")),
        "{:?}",
        zero_hit.violations
    );
    let detail_only = lane_with_mbx_log(
        "report-mbx-real-detail-only",
        REAL_MBX_1_30_DETAIL_ONLY,
        true,
        false,
    )?;
    assert!(detail_only.mbx.sessions.is_empty(), "{detail_only:?}");
    assert_eq!(detail_only.mbx.unparsed, 0, "{detail_only:?}");
    assert!(
        detail_only
            .violations
            .iter()
            .any(|line| line.contains("compiled without mbx")),
        "{:?}",
        detail_only.violations
    );
    Ok(())
}
