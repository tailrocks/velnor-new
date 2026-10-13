use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{field, map_fields, pins, string};

const SHARED_KEY_FIXTURE: &str = "qualification-mbx-v1/parallel/shared-run-123-attempt-2-seed";
const NEW_KEY_FIXTURE: &str = "qualification-mbx-v1/parallel/new-key-run-123-attempt-2-writer";

#[test]
fn fixed_observer_script_is_parseable_bash() {
    let step = super::super::api::observer_api_step(&pins());
    let fields = map_fields(&step);
    let script = string(field(fields, "run"));
    let bash_check = Command::new("bash")
        .args(["-n", "-c", script])
        .output()
        .expect("Bash is available for syntax validation");
    assert!(
        bash_check.status.success(),
        "Bash rejected the observer script: {}",
        String::from_utf8_lossy(&bash_check.stderr)
    );
    assert!(script.contains('\n'));
    assert!(!script.contains("$("));
}

#[test]
fn exact_api_fixtures_distinguish_observed_overlap_from_not_run() {
    assert_eq!(run_api_fixture(true), ("RUN".to_owned(), "6000".to_owned()));
    assert_eq!(run_api_fixture(false), ("NOT_RUN".to_owned(), "0".to_owned()));
}

fn run_api_fixture(overlap: bool) -> (String, String) {
    let root = TestDirectory::new();
    let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, overlap);
    let output = run_api_script(&root.0, &runner_temp, &jobs_path);
    assert!(
        output.status.success(),
        "fixture API script failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    verify_api_receipt(overlap, &runner_temp.join("mbx-cache-evidence/parallel-api-receipt.json"))
}

fn prepare_api_fixture(root: &std::path::Path, overlap: bool) -> (PathBuf, PathBuf) {
    let runner_temp = root.join("runner-temp");
    let input = runner_temp.join("mbx-parallel-input");
    let evidence = runner_temp.join("mbx-cache-evidence");
    let fake_bin = root.join("bin");
    fs::create_dir_all(&input).expect("create runner temp input");
    fs::create_dir(&evidence).expect("create private evidence fixture");
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700))
        .expect("make evidence fixture private");
    fs::create_dir_all(&fake_bin).expect("create fixture command directory");

    write_receipts(&input, SHARED_KEY_FIXTURE, NEW_KEY_FIXTURE);
    let jobs_path = root.join("jobs.json");
    let seed_cache_path = root.join("seed-cache.json");
    let new_cache_path = root.join("new-cache.json");
    fs::write(&jobs_path, api_jobs_fixture(overlap)).expect("write jobs API fixture");
    fs::write(&seed_cache_path, cache_fixture(101, SHARED_KEY_FIXTURE))
        .expect("write seed cache fixture");
    fs::write(&new_cache_path, cache_fixture(202, NEW_KEY_FIXTURE))
        .expect("write new cache fixture");
    write_fake_gh(&fake_bin.join("gh"));
    (runner_temp, jobs_path)
}

fn run_api_script(root: &std::path::Path, runner_temp: &std::path::Path, jobs_path: &std::path::Path) -> std::process::Output {
    let fake_bin = root.join("bin");
    let seed_cache_path = root.join("seed-cache.json");
    let new_cache_path = root.join("new-cache.json");
    let script = observer_script();
    let mut path_entries = vec![fake_bin];
    path_entries.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(path_entries).expect("join fixture PATH");
    Command::new("bash")
        .args(["-c", &script])
        .env("PATH", path)
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_RUN_ID", "123")
        .env("GITHUB_RUN_ATTEMPT", "2")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REPOSITORY", "org/repo")
        .env("GITHUB_WORKFLOW_REF", "org/repo/.github/workflows/qualification.yml@refs/heads/main")
        .env("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .env("MBX_EXPECTED_ACTION_REF", format!("jdx/mr-boxington-action@{}", "c".repeat(40)))
        .env("MBX_EXPECTED_VERSION", "1.22.0")
        .env("MBX_EXPECTED_RUST_VERSION", "1.98.1")
        .env("MBX_EXPECTED_SHARED_SCOPE", "qualification-mbx-v1/parallel/shared")
        .env("MBX_EXPECTED_NEW_KEY_SCOPE", "qualification-mbx-v1/parallel/new-key")
        .env("MBX_PARALLEL_FIXTURE_JOBS", jobs_path)
        .env("MBX_PARALLEL_FIXTURE_SEED_CACHE", &seed_cache_path)
        .env("MBX_PARALLEL_FIXTURE_NEW_CACHE", &new_cache_path)
        .env("MBX_PARALLEL_FIXTURE_SEED_KEY", SHARED_KEY_FIXTURE)
        .env("MBX_PARALLEL_FIXTURE_NEW_KEY", NEW_KEY_FIXTURE)
        .output()
        .expect("run API receipt script against fixtures")
}

fn verify_api_receipt(overlap: bool, receipt: &std::path::Path) -> (String, String) {
    let classification = jq_value(".classification", receipt);
    let duration = jq_value(".overlap_duration_ms", receipt);
    assert_eq!(
        jq_value(".seed.save_completed_at", receipt),
        "2026-10-04T00:00:03Z"
    );
    let expected_starts = if overlap {
        "2026-10-04T00:00:10Z,2026-10-04T00:00:11Z,2026-10-04T00:00:12Z"
    } else {
        "2026-10-04T00:00:10Z,2026-10-04T00:00:18Z,2026-10-04T00:00:23Z"
    };
    assert_eq!(
        jq_value(
            ".parallel_intervals | map(.restore_started_at) | join(\",\")",
            receipt
        ),
        expected_starts
    );
    let expected_ends = if overlap {
        "2026-10-04T00:00:20Z,2026-10-04T00:00:18Z,2026-10-04T00:00:23Z"
    } else {
        "2026-10-04T00:00:15Z,2026-10-04T00:00:22Z,2026-10-04T00:00:24Z"
    };
    assert_eq!(
        jq_value(
            ".parallel_intervals | map(.build_completed_at) | join(\",\")",
            receipt
        ),
        expected_ends
    );
    assert_eq!(jq_value(".exact_cache_entries | length", receipt), "2");
    assert_eq!(
        jq_value(".exact_cache_entries | map(.id) | join(\",\")", receipt),
        "101,202"
    );
    (classification, duration)
}

fn observer_script() -> String {
    let step = super::super::api::observer_api_step(&pins());
    string(field(map_fields(&step), "run")).to_owned()
}

fn write_receipts(input: &std::path::Path, shared_key: &str, new_key: &str) {
    let receipt_dir = ["seed", "reader-a", "reader-b", "new-key-writer"];
    for role in receipt_dir {
        let directory = input.join(role);
        fs::create_dir_all(&directory).expect("create receipt fixture directory");
    }
    let seed = receipt_json(ReceiptFixture {
        job: "mbx-parallel-seed",
        role: "seed",
        scope: "qualification-mbx-v1/parallel/shared",
        key: shared_key,
        hit: "false",
        matched_key: "null".to_owned(),
        imported_objects: 0,
        cached_compilations: 0,
        export_ready: "true",
        save_outcome: "success",
    });
    let reader_a = receipt_json(ReceiptFixture {
        job: "mbx-parallel-reader-a",
        role: "reader-a",
        scope: "qualification-mbx-v1/parallel/shared",
        key: shared_key,
        hit: "true",
        matched_key: format!("\"{shared_key}\""),
        imported_objects: 17,
        cached_compilations: 2,
        export_ready: "",
        save_outcome: "",
    });
    let reader_b = receipt_json(ReceiptFixture {
        job: "mbx-parallel-reader-b",
        role: "reader-b",
        scope: "qualification-mbx-v1/parallel/shared",
        key: shared_key,
        hit: "true",
        matched_key: format!("\"{shared_key}\""),
        imported_objects: 17,
        cached_compilations: 2,
        export_ready: "",
        save_outcome: "",
    });
    let writer = receipt_json(ReceiptFixture {
        job: "mbx-parallel-new-key-writer",
        role: "new-key-writer",
        scope: "qualification-mbx-v1/parallel/new-key",
        key: new_key,
        hit: "false",
        matched_key: "null".to_owned(),
        imported_objects: 0,
        cached_compilations: 0,
        export_ready: "true",
        save_outcome: "success",
    });
    for (role, json) in [
        ("seed", seed),
        ("reader-a", reader_a),
        ("reader-b", reader_b),
        ("new-key-writer", writer),
    ] {
        fs::write(input.join(role).join("cache-receipt.json"), json)
            .expect("write cache receipt fixture");
    }
}

struct ReceiptFixture<'a> {
    job: &'a str,
    role: &'a str,
    scope: &'a str,
    key: &'a str,
    hit: &'a str,
    matched_key: String,
    imported_objects: u64,
    cached_compilations: u64,
    export_ready: &'a str,
    save_outcome: &'a str,
}

fn receipt_json(fixture: ReceiptFixture<'_>) -> String {
    format!(
        r#"{{"job_id":"{job}","role":"{role}","scope":"{scope}","run_id":"123","run_attempt":"2","source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_ref":"refs/heads/main","workflow_ref":"org/repo/.github/workflows/qualification.yml@refs/heads/main","mbx_action_ref":"jdx/mr-boxington-action@{action_sha}","mbx_version":"1.22.0","rust_version":"1.98.1","primary_key":"{key}","cache_hit":"{hit}","matched_key":{matched_key},"imported_objects":{imported_objects},"cached_compilations":{cached_compilations},"export_ready":"{export_ready}","save_outcome":"{save_outcome}"}}"#,
        job = fixture.job,
        role = fixture.role,
        scope = fixture.scope,
        key = fixture.key,
        hit = fixture.hit,
        matched_key = fixture.matched_key,
        imported_objects = fixture.imported_objects,
        cached_compilations = fixture.cached_compilations,
        export_ready = fixture.export_ready,
        save_outcome = fixture.save_outcome,
        action_sha = "c".repeat(40),
    )
}

fn api_jobs_fixture(overlap: bool) -> String {
    let (a_start, a_end, b_start, b_end, writer_start, writer_end) = if overlap {
        ("00:00:10", "00:00:20", "00:00:11", "00:00:18", "00:00:12", "00:00:23")
    } else {
        ("00:00:10", "00:00:15", "00:00:18", "00:00:22", "00:00:23", "00:00:24")
    };
    let seed_steps = r#"[{"name":"Save MBX single bundle","status":"completed","conclusion":"success","started_at":"2026-10-04T00:00:02Z","completed_at":"2026-10-04T00:00:03Z"}]"#;
    let reader_a_steps = interval_steps(a_start, a_end);
    let reader_b_phase_steps = interval_steps(b_start, b_end);
    let writer_steps = interval_steps(writer_start, writer_end);
    let seed = job_record("MBX parallel / seed", 11, "00:00:04", seed_steps);
    let reader_a = job_record("MBX parallel / reader-a", 12, a_end, &reader_a_steps);
    let reader_b = job_record(
        "MBX parallel / reader-b",
        13,
        b_end,
        &reader_b_phase_steps,
    );
    let writer = job_record("MBX parallel / new-key-writer", 14, writer_end, &writer_steps);
    format!(r#"{{"total_count":4,"jobs":[{seed},{reader_a},{reader_b},{writer}]}}"#)
}

fn interval_steps(start: &str, end: &str) -> String {
    format!(
        r#"[{{"name":"Restore MBX single bundle","status":"completed","conclusion":"success","started_at":"2026-10-04T{start}Z","completed_at":"2026-10-04T{start}Z"}},{{"name":"Compile MBX cache probe","status":"completed","conclusion":"success","started_at":"2026-10-04T{start}Z","completed_at":"2026-10-04T{end}Z"}}]"#
    )
}

fn job_record(name: &str, id: u64, end: &str, steps: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","status":"completed","conclusion":"success","completed_at":"2026-10-04T{end}Z","steps":{steps}}}"#
    )
}

fn cache_fixture(id: u64, key: &str) -> String {
    format!(
        r#"{{"actions_caches":[{{"id":{id},"key":"{key}","ref":"refs/heads/main","version":"cache-v2","size_in_bytes":1234,"created_at":"2026-10-04T00:00:00Z"}}]}}"#
    )
}

fn write_fake_gh(path: &std::path::Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
case "$*" in
  *"/attempts/$GITHUB_RUN_ATTEMPT/jobs"*) cat "$MBX_PARALLEL_FIXTURE_JOBS" ;;
  *"key=$MBX_PARALLEL_FIXTURE_SEED_KEY"*) cat "$MBX_PARALLEL_FIXTURE_SEED_CACHE" ;;
  *"key=$MBX_PARALLEL_FIXTURE_NEW_KEY"*) cat "$MBX_PARALLEL_FIXTURE_NEW_CACHE" ;;
  *) echo 'unexpected fixture API request' >&2; exit 97 ;;
esac
"#,
    )
    .expect("write fixture gh command");
    let mut permissions = fs::metadata(path)
        .expect("read fixture gh metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make fixture gh executable");
}

fn jq_value(filter: &str, path: &std::path::Path) -> String {
    let output = Command::new("jq")
        .args(["-er", filter])
        .arg(path)
        .output()
        .expect("jq is available for API receipt validation");
    assert!(
        output.status.success(),
        "jq rejected the API receipt: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("jq output is UTF-8")
        .trim()
        .to_owned()
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "mbx-parallel-api-fixture-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create unique API fixture directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("remove API fixture directory {}: {error}", self.0.display());
        }
    }
}
