// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::verification_tools_fixture as fixture;

const NEXT: &str = "aqua:nextest-rs/nextest/cargo-nextest";

#[test]
fn verification_task_rejects_mise_lock_v3() -> Result<(), Box<dyn Error>> {
    let lock = fixture::LOCK.replace("lockfile_version = 4", "lockfile_version = 3");
    expect_prepare_rejection(&lock, "verification_mise_lock_root")
}

#[test]
fn build_task_rejects_unsupported_root_lock_versions() -> Result<(), Box<dyn Error>> {
    for version in [3, 999] {
        let lock = lock_with_root_version(version)?;
        let repo = fixture::build_task_repo_with_lock(&lock)?;
        expect_build_task_rejection(&repo)?;
    }
    Ok(())
}

#[test]
fn build_task_rejects_unsupported_source_lock_versions() -> Result<(), Box<dyn Error>> {
    for version in [3, 999] {
        let lock = source_lock_with_root_version(version)?;
        let repo = fixture::build_task_repo_with_source_lock(&lock)?;
        expect_build_task_rejection(&repo)?;
    }
    Ok(())
}

#[test]
fn verification_task_rejects_non_string_checksum_source() -> Result<(), Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"][NEXT][0]["platforms.linux-x64"]["checksum_source"] = toml::Value::Integer(4);
    expect_prepare_rejection(
        &toml::to_string(&lock)?,
        "verification_tool_platform_artifact",
    )
}

#[test]
fn verification_task_rejects_unapproved_checksum_source() -> Result<(), Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"][NEXT][0]["platforms.linux-x64"]["checksum_source"] =
        toml::Value::String("untrusted-source".to_owned());
    expect_prepare_rejection(
        &toml::to_string(&lock)?,
        "verification_tool_platform_artifact",
    )
}

#[test]
fn verification_task_rejects_unknown_selected_artifact_field() -> Result<(), Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"][NEXT][0]["platforms.linux-x64"]
        .as_table_mut()
        .ok_or("missing selected artifact")?
        .insert(
            "unexpected_artifact_field".to_owned(),
            toml::Value::Boolean(true),
        );
    expect_prepare_rejection(
        &toml::to_string(&lock)?,
        "verification_tool_platform_artifact",
    )
}

#[test]
fn verification_task_rejects_invalid_selected_checksum_after_v4_admission()
-> Result<(), Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"][NEXT][0]["platforms.linux-x64"]["checksum"] =
        toml::Value::String("sha256:not-a-digest".to_owned());
    expect_finalized_rejection(&toml::to_string(&lock)?, "build_task_artifact:aqua:")
}

#[test]
fn verification_task_rejects_invalid_selected_url_after_v4_admission() -> Result<(), Box<dyn Error>>
{
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"][NEXT][0]["platforms.linux-x64"]["url"] =
        toml::Value::String("file:///tmp/tool.tar.gz".to_owned());
    expect_finalized_rejection(&toml::to_string(&lock)?, "build_task_artifact:aqua:")
}

#[test]
fn verification_task_rejects_invalid_selected_signer_after_v4_admission()
-> Result<(), Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"]["hk"][0]["platforms.linux-x64"]["signer"] =
        toml::Value::String("unsafe signer".to_owned());
    expect_finalized_rejection(&toml::to_string(&lock)?, "build_task_artifact:hk")
}

fn expect_prepare_rejection(lock: &str, expected: &str) -> Result<(), Box<dyn Error>> {
    let repo = fixture::repo(fixture::REGEX, lock)?;
    let error = prepare(repo.path())
        .err()
        .ok_or("invalid lock was accepted during prepare")?;
    assert!(error.to_string().contains(expected), "{error}");
    Ok(())
}

fn expect_finalized_rejection(lock: &str, expected: &str) -> Result<(), Box<dyn Error>> {
    let repo = fixture::repo(fixture::REGEX, lock)?;
    let error = match prepare(repo.path()) {
        Ok(prepared) => finalized_jobs(&prepared)
            .err()
            .ok_or("invalid artifact was accepted during finalization")?,
        Err(error) => error,
    };
    assert!(error.to_string().contains(expected), "{error}");
    Ok(())
}

fn lock_with_root_version(version: i64) -> Result<String, Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["lockfile_version"] = toml::Value::Integer(version);
    Ok(toml::to_string(&lock)?)
}

fn source_lock_with_root_version(version: i64) -> Result<String, Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["lockfile_version"] = toml::Value::Integer(version);
    lock["tools"]
        .as_table_mut()
        .ok_or("missing source lock tools")?
        .retain(|key, _| key == NEXT);
    Ok(toml::to_string(&lock)?)
}

fn expect_build_task_rejection(repo: &tempfile::TempDir) -> Result<(), Box<dyn Error>> {
    let error = prepare(repo.path())
        .err()
        .ok_or("BuildTask accepted an unsupported Mise lock version")?;
    assert!(
        error.to_string().contains("build_task_mise_lock_root"),
        "{error}"
    );
    Ok(())
}
