use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{NEXT_FIXTURE_ID, RUSTC_IDENTITY, SOURCE_SHA};

#[path = "schema2_mbx_parallel_probe_api_stock_fixture.rs"]
mod stock_fixture;
pub(super) use stock_fixture::write_stock_restore_fixtures;

pub(super) fn write_receipts(
    input: &std::path::Path,
    shared_key: &str,
    shared_prefix: &str,
    new_key: &str,
    new_prefix: &str,
    runner_os: &str,
    runner_arch: &str,
) {
    let context = ReceiptContext {
        shared_key,
        shared_prefix,
        new_key,
        new_prefix,
        runner_os,
        runner_arch,
    };
    for (role, fixture) in [
        ("seed", ReceiptFixture::seed(&context)),
        (
            "reader-a",
            ReceiptFixture::reader(&context, "mbx-parallel-reader-a", "reader-a"),
        ),
        (
            "reader-b",
            ReceiptFixture::reader(&context, "mbx-parallel-reader-b", "reader-b"),
        ),
        ("new-key-writer", ReceiptFixture::writer(&context)),
    ] {
        let directory = input.join(role);
        fs::create_dir_all(&directory).expect("create receipt fixture directory");
        fs::write(
            input.join(role).join("cache-receipt.json"),
            receipt_json(&fixture),
        )
        .expect("write cache receipt fixture");
    }
}

struct ReceiptContext<'a> {
    shared_key: &'a str,
    shared_prefix: &'a str,
    new_key: &'a str,
    new_prefix: &'a str,
    runner_os: &'a str,
    runner_arch: &'a str,
}

struct ReceiptFixture<'a> {
    job: &'a str,
    role: &'a str,
    scope: &'a str,
    key: &'a str,
    cache_prefix: &'a str,
    runner_os: &'a str,
    runner_arch: &'a str,
    hit: &'a str,
    matched_key: String,
    restore_primary_key: &'a str,
    restore_conclusion: &'a str,
    imported_objects: u64,
    cached_compilations: u64,
    export_ready: &'a str,
    save_outcome: &'a str,
}

impl<'a> ReceiptFixture<'a> {
    fn seed(context: &ReceiptContext<'a>) -> Self {
        Self {
            job: "mbx-parallel-seed",
            role: "seed",
            scope: "qualification-mbx-v1/parallel/shared",
            key: context.shared_key,
            cache_prefix: context.shared_prefix,
            runner_os: context.runner_os,
            runner_arch: context.runner_arch,
            hit: "",
            matched_key: "\"\"".to_owned(),
            restore_primary_key: context.shared_key,
            restore_conclusion: "success",
            imported_objects: 0,
            cached_compilations: 0,
            export_ready: "true",
            save_outcome: "success",
        }
    }

    fn reader(context: &ReceiptContext<'a>, job: &'static str, role: &'static str) -> Self {
        Self {
            job,
            role,
            scope: "qualification-mbx-v1/parallel/shared",
            key: context.shared_key,
            cache_prefix: context.shared_prefix,
            runner_os: context.runner_os,
            runner_arch: context.runner_arch,
            hit: "true",
            matched_key: format!("\"{}\"", context.shared_key),
            restore_primary_key: context.shared_key,
            restore_conclusion: "success",
            imported_objects: 17,
            cached_compilations: 2,
            export_ready: "",
            save_outcome: "",
        }
    }

    fn writer(context: &ReceiptContext<'a>) -> Self {
        Self {
            job: "mbx-parallel-new-key-writer",
            role: "new-key-writer",
            scope: "qualification-mbx-v1/parallel/new-key",
            key: context.new_key,
            cache_prefix: context.new_prefix,
            runner_os: context.runner_os,
            runner_arch: context.runner_arch,
            hit: "",
            matched_key: "\"\"".to_owned(),
            restore_primary_key: context.new_key,
            restore_conclusion: "success",
            imported_objects: 0,
            cached_compilations: 0,
            export_ready: "true",
            save_outcome: "success",
        }
    }
}

fn receipt_json(fixture: &ReceiptFixture<'_>) -> String {
    format!(
        r#"{{"receipt_status":"provisional","job_id":"{job}","role":"{role}","scope":"{scope}","run_id":"123","run_attempt":"2","source_sha":"{source_sha}","source_ref":"refs/heads/main","workflow_ref":"tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main","runner_os":"{runner_os}","runner_arch":"{runner_arch}","mbx_action_ref":"jdx/mr-boxington-action@{action_sha}","mbx_version":"1.22.0","rust_version":"1.98.1","generation":"velnor-mbx-1.22.0","rustc_identity":"{rustc_identity}","cache_prefix":"{cache_prefix}","primary_key":"{key}","derived_primary_key":"{key}","restore_primary_key":"{restore_primary_key}","restore_conclusion":"{restore_conclusion}","cache_hit":"{hit}","matched_key":{matched_key},"imported_objects":{imported_objects},"cached_compilations":{cached_compilations},"export_ready":"{export_ready}","save_outcome":"{save_outcome}"}}"#,
        job = fixture.job,
        role = fixture.role,
        scope = fixture.scope,
        source_sha = SOURCE_SHA,
        key = fixture.key,
        cache_prefix = fixture.cache_prefix,
        runner_os = fixture.runner_os,
        runner_arch = fixture.runner_arch,
        hit = fixture.hit,
        matched_key = fixture.matched_key,
        restore_primary_key = fixture.restore_primary_key,
        restore_conclusion = fixture.restore_conclusion,
        imported_objects = fixture.imported_objects,
        cached_compilations = fixture.cached_compilations,
        export_ready = fixture.export_ready,
        save_outcome = fixture.save_outcome,
        action_sha = "c".repeat(40),
        rustc_identity = RUSTC_IDENTITY,
    )
}

pub(super) fn api_jobs_fixture(overlap: bool) -> String {
    let (a_start, a_end, b_start, b_end, writer_start, writer_end) = if overlap {
        (
            "00:00:10", "00:00:20", "00:00:11", "00:00:18", "00:00:12", "00:00:23",
        )
    } else {
        (
            "00:00:10", "00:00:15", "00:00:18", "00:00:22", "00:00:23", "00:00:24",
        )
    };
    let seed_steps = r#"[{"name":"Restore MBX single bundle","status":"completed","conclusion":"success","started_at":"2026-10-04T00:00:01Z","completed_at":"2026-10-04T00:00:02Z"},{"name":"Save MBX single bundle","status":"completed","conclusion":"success","started_at":"2026-10-04T00:00:02Z","completed_at":"2026-10-04T00:00:03Z"}]"#;
    let reader_a_steps = interval_steps(a_start, a_end);
    let reader_b_phase_steps = interval_steps(b_start, b_end);
    let writer_steps = interval_steps(writer_start, writer_end);
    let seed = job_record("MBX parallel / seed", 11, "00:00:04", seed_steps);
    let reader_a = job_record("MBX parallel / reader-a", 12, a_end, &reader_a_steps);
    let reader_b = job_record("MBX parallel / reader-b", 13, b_end, &reader_b_phase_steps);
    let writer = job_record(
        "MBX parallel / new-key-writer",
        14,
        writer_end,
        &writer_steps,
    );
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

pub(super) fn cache_fixture(id: u64, key: &str) -> String {
    format!(
        r#"{{"actions_caches":[{{"id":{id},"key":"{key}","ref":"refs/heads/main","version":"cache-v2","size_in_bytes":1234,"created_at":"2026-10-04T00:00:00Z"}}]}}"#
    )
}

pub(super) fn write_fake_gh(path: &std::path::Path) {
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

pub(super) fn jq_value(filter: &str, path: &std::path::Path) -> String {
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

pub(super) struct TestDirectory(pub(super) PathBuf);

impl TestDirectory {
    pub(super) fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos();
        loop {
            let id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "mbx-parallel-api-fixture-{}-{nonce}-{id}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create unique API fixture directory: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("remove API fixture directory {}: {error}", self.0.display());
        }
    }
}
