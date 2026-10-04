use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicU64;

use super::{field, map_fields, pins, string};

#[path = "schema2_mbx_parallel_probe_api_fixture.rs"]
mod fixture_data;

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const RUSTC_IDENTITY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHARED_SCOPE_HASH: &str = "d56a34bb60253d84ce8cf192d43a64a8d2a31c7941da51c6fad482e6e13a0a12";
const NEW_SCOPE_HASH: &str = "80d264d20dca8b9e9d723ab1dd7f8116255c29520be42966afa4af4c3cd0b4f9";
static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

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
    assert_eq!(
        run_api_fixture(false),
        ("NOT_RUN".to_owned(), "0".to_owned())
    );
}

#[test]
fn api_accepts_exact_arm64_keys_from_the_supported_key_builder() {
    let root = fixture_data::TestDirectory::new();
    let (runner_temp, jobs_path) =
        prepare_api_fixture_for_platform(&root.0, true, "Linux", "ARM64", "arm64");
    let output =
        run_api_script_for_platform(&root.0, &runner_temp, &jobs_path, "Linux", "ARM64", "arm64");
    assert!(
        output.status.success(),
        "ARM64 fixture API script failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        verify_api_receipt(
            true,
            &runner_temp.join("mbx-cache-evidence/parallel-api-receipt.json"),
        ),
        ("RUN".to_owned(), "6000".to_owned())
    );
}

#[test]
fn api_rejects_unsupported_runner_architecture() {
    let root = fixture_data::TestDirectory::new();
    let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
    let output =
        run_api_script_for_platform(&root.0, &runner_temp, &jobs_path, "Linux", "RISCV64", "x64");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported runner platform"));
}

#[test]
fn api_rejects_stale_seed_when_new_writer_key_is_current() {
    assert_api_fixture_rejected(KeyMutation::StaleSeed);
}

#[test]
fn api_rejects_stale_new_writer_when_seed_key_is_current() {
    assert_api_fixture_rejected(KeyMutation::StaleWriter);
}

#[test]
fn api_rejects_wrong_seed_sha_when_new_writer_sha_is_current() {
    assert_api_fixture_rejected(KeyMutation::WrongSeedSha);
}

#[test]
fn api_rejects_wrong_new_writer_sha_when_seed_sha_is_current() {
    assert_api_fixture_rejected(KeyMutation::WrongWriterSha);
}

#[test]
fn api_rejects_near_match_scope_hash_with_current_run_attempt_and_sha() {
    assert_api_fixture_rejected(KeyMutation::NearMatchSharedScopeHash);
}

#[test]
fn api_rejects_malformed_receipt_scope() {
    assert_api_fixture_rejected(KeyMutation::MalformedSeedScope);
}

fn assert_api_fixture_rejected(mutation: KeyMutation) {
    let root = fixture_data::TestDirectory::new();
    let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
    mutate_fixture_receipt(&runner_temp, mutation);
    let output = run_api_script(&root.0, &runner_temp, &jobs_path);
    assert!(
        !output.status.success(),
        "API script accepted key mutation {mutation:?}"
    );
}

#[derive(Debug, Clone, Copy)]
enum KeyMutation {
    StaleSeed,
    StaleWriter,
    WrongSeedSha,
    WrongWriterSha,
    NearMatchSharedScopeHash,
    MalformedSeedScope,
}

fn mutate_fixture_receipt(runner_temp: &std::path::Path, mutation: KeyMutation) {
    let input = runner_temp.join("mbx-parallel-input");
    let shared_prefix = cache_prefix(SHARED_SCOPE_HASH, "123", "2");
    let shared_key = format!("{shared_prefix}{SOURCE_SHA}");
    let new_prefix = cache_prefix(NEW_SCOPE_HASH, "123", "2");
    let new_key = format!("{new_prefix}{SOURCE_SHA}");
    match mutation {
        KeyMutation::StaleSeed => {
            let stale_prefix = cache_prefix(SHARED_SCOPE_HASH, "122", "2");
            let stale_key = format!("{stale_prefix}{SOURCE_SHA}");
            let receipt = input.join("seed/cache-receipt.json");
            replace_receipt_field(&receipt, "cache_prefix", &shared_prefix, &stale_prefix);
            replace_receipt_field(&receipt, "primary_key", &shared_key, &stale_key);
        }
        KeyMutation::StaleWriter => {
            let stale_prefix = cache_prefix(NEW_SCOPE_HASH, "122", "2");
            let stale_key = format!("{stale_prefix}{SOURCE_SHA}");
            let receipt = input.join("new-key-writer/cache-receipt.json");
            replace_receipt_field(&receipt, "cache_prefix", &new_prefix, &stale_prefix);
            replace_receipt_field(&receipt, "primary_key", &new_key, &stale_key);
        }
        KeyMutation::WrongSeedSha => {
            let wrong_key = format!("{shared_prefix}{OTHER_SHA}");
            replace_receipt_field(
                &input.join("seed/cache-receipt.json"),
                "primary_key",
                &shared_key,
                &wrong_key,
            );
        }
        KeyMutation::WrongWriterSha => {
            let wrong_key = format!("{new_prefix}{OTHER_SHA}");
            replace_receipt_field(
                &input.join("new-key-writer/cache-receipt.json"),
                "primary_key",
                &new_key,
                &wrong_key,
            );
        }
        KeyMutation::NearMatchSharedScopeHash => {
            let near_hash = format!("{}3", &SHARED_SCOPE_HASH[..SHARED_SCOPE_HASH.len() - 1]);
            let near_prefix = cache_prefix(&near_hash, "123", "2");
            let near_key = format!("{near_prefix}{SOURCE_SHA}");
            let receipt = input.join("seed/cache-receipt.json");
            replace_receipt_field(&receipt, "cache_prefix", &shared_prefix, &near_prefix);
            replace_receipt_field(&receipt, "primary_key", &shared_key, &near_key);
        }
        KeyMutation::MalformedSeedScope => replace_receipt_field(
            &input.join("seed/cache-receipt.json"),
            "scope",
            "qualification-mbx-v1/parallel/shared",
            "qualification-mbx-v1/parallel/shared-near",
        ),
    }
}

fn replace_receipt_field(path: &std::path::Path, field: &str, old: &str, new: &str) {
    let contents = fs::read_to_string(path).expect("read receipt fixture for mutation");
    let old_field = format!("\"{field}\":\"{old}\"");
    let new_field = format!("\"{field}\":\"{new}\"");
    assert_eq!(contents.matches(&old_field).count(), 1);
    fs::write(path, contents.replace(&old_field, &new_field))
        .expect("write mutated receipt fixture");
}

fn cache_prefix(scope_hash: &str, run_id: &str, attempt: &str) -> String {
    cache_prefix_for_arch(scope_hash, run_id, attempt, "x64")
}

fn cache_prefix_for_arch(scope_hash: &str, run_id: &str, attempt: &str, key_arch: &str) -> String {
    format!(
        "linux-{key_arch}-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-{RUSTC_IDENTITY}-scope-{scope_hash}-run-{run_id}-attempt-{attempt}-"
    )
}

fn cache_key_fixture_for_arch(scope_hash: &str, key_arch: &str) -> String {
    format!(
        "{}{SOURCE_SHA}",
        cache_prefix_for_arch(scope_hash, "123", "2", key_arch)
    )
}

fn run_api_fixture(overlap: bool) -> (String, String) {
    let root = fixture_data::TestDirectory::new();
    let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, overlap);
    let output = run_api_script(&root.0, &runner_temp, &jobs_path);
    assert!(
        output.status.success(),
        "fixture API script failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    verify_api_receipt(
        overlap,
        &runner_temp.join("mbx-cache-evidence/parallel-api-receipt.json"),
    )
}

fn prepare_api_fixture(root: &std::path::Path, overlap: bool) -> (PathBuf, PathBuf) {
    prepare_api_fixture_for_platform(root, overlap, "Linux", "X64", "x64")
}

fn prepare_api_fixture_for_platform(
    root: &std::path::Path,
    overlap: bool,
    runner_os: &str,
    runner_arch: &str,
    key_arch: &str,
) -> (PathBuf, PathBuf) {
    let runner_temp = root.join("runner-temp");
    let input = runner_temp.join("mbx-parallel-input");
    let evidence = runner_temp.join("mbx-cache-evidence");
    let fake_bin = root.join("bin");
    fs::create_dir_all(&input).expect("create runner temp input");
    fs::create_dir(&evidence).expect("create private evidence fixture");
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700))
        .expect("make evidence fixture private");
    fs::create_dir_all(&fake_bin).expect("create fixture command directory");

    let shared_prefix = cache_prefix_for_arch(SHARED_SCOPE_HASH, "123", "2", key_arch);
    let new_prefix = cache_prefix_for_arch(NEW_SCOPE_HASH, "123", "2", key_arch);
    let shared_key = format!("{shared_prefix}{SOURCE_SHA}");
    let new_key = format!("{new_prefix}{SOURCE_SHA}");
    fixture_data::write_receipts(
        &input,
        &shared_key,
        &shared_prefix,
        &new_key,
        &new_prefix,
        runner_os,
        runner_arch,
    );
    let jobs_path = root.join("jobs.json");
    let seed_cache_path = root.join("seed-cache.json");
    let new_cache_path = root.join("new-cache.json");
    fs::write(&jobs_path, fixture_data::api_jobs_fixture(overlap)).expect("write jobs API fixture");
    fs::write(
        &seed_cache_path,
        fixture_data::cache_fixture(101, &shared_key),
    )
    .expect("write seed cache fixture");
    fs::write(&new_cache_path, fixture_data::cache_fixture(202, &new_key))
        .expect("write new cache fixture");
    fixture_data::write_fake_gh(&fake_bin.join("gh"));
    (runner_temp, jobs_path)
}

fn run_api_script(
    root: &std::path::Path,
    runner_temp: &std::path::Path,
    jobs_path: &std::path::Path,
) -> std::process::Output {
    run_api_script_for_platform(root, runner_temp, jobs_path, "Linux", "X64", "x64")
}

fn run_api_script_for_platform(
    root: &std::path::Path,
    runner_temp: &std::path::Path,
    jobs_path: &std::path::Path,
    runner_os: &str,
    runner_arch: &str,
    key_arch: &str,
) -> std::process::Output {
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
        .env(
            "GITHUB_WORKFLOW_REF",
            "org/repo/.github/workflows/qualification.yml@refs/heads/main",
        )
        .env("GITHUB_SHA", SOURCE_SHA)
        .env("RUNNER_OS", runner_os)
        .env("RUNNER_ARCH", runner_arch)
        .env(
            "MBX_EXPECTED_ACTION_REF",
            format!("jdx/mr-boxington-action@{}", "c".repeat(40)),
        )
        .env("MBX_EXPECTED_VERSION", "1.22.0")
        .env("MBX_EXPECTED_RUST_VERSION", "1.98.1")
        .env(
            "MBX_EXPECTED_SHARED_SCOPE",
            "qualification-mbx-v1/parallel/shared",
        )
        .env(
            "MBX_EXPECTED_NEW_KEY_SCOPE",
            "qualification-mbx-v1/parallel/new-key",
        )
        .env("MBX_PARALLEL_FIXTURE_JOBS", jobs_path)
        .env("MBX_PARALLEL_FIXTURE_SEED_CACHE", &seed_cache_path)
        .env("MBX_PARALLEL_FIXTURE_NEW_CACHE", &new_cache_path)
        .env(
            "MBX_PARALLEL_FIXTURE_SEED_KEY",
            cache_key_fixture_for_arch(SHARED_SCOPE_HASH, key_arch),
        )
        .env(
            "MBX_PARALLEL_FIXTURE_NEW_KEY",
            cache_key_fixture_for_arch(NEW_SCOPE_HASH, key_arch),
        )
        .output()
        .expect("run API receipt script against fixtures")
}

fn verify_api_receipt(overlap: bool, receipt: &std::path::Path) -> (String, String) {
    let classification = fixture_data::jq_value(".classification", receipt);
    let duration = fixture_data::jq_value(".overlap_duration_ms", receipt);
    assert_eq!(
        fixture_data::jq_value(".seed.save_completed_at", receipt),
        "2026-10-04T00:00:03Z"
    );
    let expected_starts = if overlap {
        "2026-10-04T00:00:10Z,2026-10-04T00:00:11Z,2026-10-04T00:00:12Z"
    } else {
        "2026-10-04T00:00:10Z,2026-10-04T00:00:18Z,2026-10-04T00:00:23Z"
    };
    assert_eq!(
        fixture_data::jq_value(
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
        fixture_data::jq_value(
            ".parallel_intervals | map(.build_completed_at) | join(\",\")",
            receipt
        ),
        expected_ends
    );
    assert_eq!(
        fixture_data::jq_value(".exact_cache_entries | length", receipt),
        "2"
    );
    assert_eq!(
        fixture_data::jq_value(".exact_cache_entries | map(.id) | join(\",\")", receipt),
        "101,202"
    );
    (classification, duration)
}

fn observer_script() -> String {
    let step = super::super::api::observer_api_step(&pins());
    string(field(map_fields(&step), "run")).to_owned()
}
