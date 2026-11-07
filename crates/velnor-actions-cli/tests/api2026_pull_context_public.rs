// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[cfg(unix)]
#[path = "api2026_pull_context_fixture/capture_race_tests.rs"]
mod capture_race_tests;
#[path = "api2026_pull_context_fixture.rs"]
mod fixture;
#[path = "api2026_pull_context_fixture/non_inputs.rs"]
mod reviewed_input_helper;

fn add_non_input(config: &str, pattern: &str) -> Result<String, Box<dyn Error>> {
    let mut document: toml::Value = toml::from_str(config)?;
    let rust = document
        .get_mut("stacks")
        .and_then(|v| v.get_mut("rust"))
        .and_then(toml::Value::as_table_mut)
        .ok_or("missing Rust stack")?;
    let inputs = rust
        .entry("non_inputs")
        .or_insert_with(|| toml::Value::Array(Vec::new()));
    inputs
        .as_array_mut()
        .ok_or("invalid non_inputs")?
        .push(pattern.into());
    Ok(toml::to_string(&document)?)
}

#[test]
fn sparse_fixture_rejects_exclusion_overlapping_a_package() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let clone = temporary.path().join("repo");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace root missing")?;
    let status = std::process::Command::new("/usr/bin/git")
        .args(["clone", "--quiet", "--shared"])
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .arg(root)
        .arg(&clone)
        .status()?;
    assert!(status.success());
    let status = std::process::Command::new("/usr/bin/git")
        .current_dir(&clone)
        .args([
            "remote",
            "set-url",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ])
        .status()?;
    assert!(status.success());
    let path = clone.join(".velnor/config.toml");
    let config = std::fs::read_to_string(&path)?;
    let invalid = add_non_input(&config, "crates/**")?;
    assert_ne!(config, invalid);
    std::fs::write(path, invalid)?;
    let error = reviewed_input_helper::reviewed_non_inputs_from(&clone)
        .expect_err("package exclusion must be refused");
    assert!(
        error
            .to_string()
            .contains("non_input_overlaps_claim:package_dir"),
        "{error}"
    );
    Ok(())
}

#[test]
fn frozen_capture_preserves_indexed_and_dirty_source() -> Result<(), Box<dyn Error>> {
    let status = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-B"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/api2026_pull_context_fixture/raw_capture_probe.py"
        ))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn sparse_source_snapshot_respects_only_reviewed_exclusions() -> Result<(), Box<dyn Error>> {
    let status = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/api2026_pull_context_fixture/sparse_snapshot_probe.py"
        ))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn isolated_source_snapshot_preserves_tracked_inputs_and_rejects_drift()
-> Result<(), Box<dyn Error>> {
    let status = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/api2026_pull_context_fixture/snapshot_probe.py"
        ))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn isolated_fixture_tools_match_qualified_catalog_and_lock() -> Result<(), Box<dyn Error>> {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("api2026_pull_context_fixture/tools.json"))?;
    let lock: toml::Value = toml::from_str(include_str!("../../../mise.lock"))?;
    let nextest = &lock["tools"]["aqua:nextest-rs/nextest/cargo-nextest"][0];
    assert_eq!(
        manifest["nextest_version"].as_str(),
        nextest["version"].as_str()
    );
    for (arch, platform, gh) in [
        (
            "x86_64",
            "linux-x64",
            velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64,
        ),
        (
            "aarch64",
            "linux-arm64",
            velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_ARM64,
        ),
    ] {
        assert_eq!(manifest["gh_version"].as_str(), Some(gh.version));
        assert_eq!(manifest[arch]["tool_platform"].as_str(), Some(gh.platform));
        assert_eq!(manifest[arch]["gh_sha256"].as_str(), Some(gh.sha256));
        assert_eq!(
            manifest[arch]["gh_archive_sha256"].as_str(),
            Some(gh.archive_sha256)
        );
        let checksum = format!(
            "sha256:{}",
            manifest[arch]["nextest_archive_sha256"]
                .as_str()
                .ok_or("missing nextest checksum")?
        );
        assert_eq!(
            nextest[format!("platforms.{platform}")]["checksum"].as_str(),
            Some(checksum.as_str())
        );
    }
    let image = manifest["image"].as_str().ok_or("missing fixture image")?;
    assert!(
        include_str!("../../velnor-actions-workflow-renderer/src/release_rehearsal_metadata.sh")
            .contains(image)
    );
    Ok(())
}

#[cfg(not(debug_assertions))]
#[test]
fn release_consumer_manifest_sentinel_never_bypasses_executable_binding()
-> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReleaseSentinel,
        "release_manifest_binary_mismatch",
    )
}

#[test]
fn absent_rest_merge_sha_reaches_owned_locator_gate() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::Valid,
        "homebrew_authority_locator_missing",
    )
}

#[test]
fn reversed_api_merge_parents_are_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReversedParents,
        "package_public_pull_request_merge_commit_mismatch",
    )
}

#[test]
fn changed_merge_ref_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ChangedRef,
        "package_public_pull_request_merge_ref_changed",
    )
}

#[test]
fn github_api_http_failure_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::HttpFailure, "category=nonzero")
}

#[test]
fn github_api_stdout_limit_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::StdoutLimit, "category=stdout_limit")
}

#[test]
fn github_api_stderr_limit_reports_safe_context() -> Result<(), Box<dyn Error>> {
    fixture::run(fixture::Scenario::StderrLimit, "category=stderr_limit")
}

#[test]
fn github_api_deadline_reports_safe_context() -> Result<(), Box<dyn Error>> {
    if fixture::deadline_child()? {
        return Ok(());
    }
    fixture::run(fixture::Scenario::Deadline, "category=timeout")
}

#[test]
fn pending_mergeability_refreshes_before_trusting_merge_ref() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingReady,
        "homebrew_authority_locator_missing",
    )
}

#[test]
fn unresolved_mergeability_never_produces_a_plan() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingExhausted,
        "package_public_pull_request_mergeability_unresolved",
    )
}

#[test]
fn conflicting_pull_request_finishes_mergeability_refresh() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingFalse,
        "homebrew_authority_pull_request_context:runner_sha",
    )
}

#[test]
fn pull_request_changed_during_mergeability_refresh_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::MergePendingChanged,
        "package_public_pull_request_changed",
    )
}

#[test]
fn mergeability_polling_obeys_the_public_operation_deadline() -> Result<(), Box<dyn Error>> {
    for scenario in [
        fixture::Scenario::MergePendingDeadline,
        fixture::Scenario::MergePendingApiDeadline,
    ] {
        fixture::run(scenario, "package_public_verification_deadline")?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn release_git_fixture_reaps_timed_out_archive_reader() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/api2026_pull_context_fixture");
    let probe = r#"
import os, pathlib, subprocess, sys
sys.path.insert(0, sys.argv[1])
import driver
pid_file = pathlib.Path(sys.argv[2])
reader = [sys.executable, '-c', 'import os,pathlib,time; pathlib.Path('+repr(str(pid_file))+').write_text(str(os.getpid())); time.sleep(60)']
extractor = [sys.executable, '-c', 'import sys; sys.stdin.buffer.read()']
try:
    driver.extract_git_member(reader, extractor, timeout=1)
except (RuntimeError, subprocess.TimeoutExpired):
    pass
else:
    raise AssertionError('archive timeout accepted')
pid = int(pid_file.read_text())
try:
    os.kill(pid, 0)
except ProcessLookupError:
    pass
else:
    raise AssertionError('archive reader was not reaped')
producer = [sys.executable, '-c', 'import sys; sys.stdout.buffer.write(b"x" * (16 * 1024 * 1024 + 1))']
copy = [sys.executable, '-c', 'import shutil,sys; shutil.copyfileobj(sys.stdin.buffer, sys.stdout.buffer)']
try:
    driver.extract_git_member(producer, copy, timeout=20)
except RuntimeError as error:
    if 'exceeded its limit' not in str(error):
        raise
else:
    raise AssertionError('oversized extracted member accepted')
"#;
    let status = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-c", probe])
        .arg(fixture)
        .arg(temp.path().join("reader.pid"))
        .status()?;
    assert!(status.success());
    Ok(())
}
