// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
    time::Instant,
};
use velnor_actions_mise::{BoundedCommandOutput, BoundedCommandStatus};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(super) fn merge_group_identity(
    binary: &str,
    root: &Path,
    temp: &Path,
    deadline: Instant,
) -> Result<()> {
    let sha = std::env::var("GITHUB_SHA")?;
    for (branch, expected) in [
        ("main", "native_service_descriptor"),
        ("other", "native_qualification_workflow_identity"),
    ] {
        record_absent(temp)?;
        let event_ref = format!("refs/heads/gh-readonly-queue/{branch}/pr-1-{sha}");
        let output = super::bounded(
            Command::new(binary)
                .args(["image-maintenance", "run-native-qualification", "--root"])
                .arg(root)
                .arg("--runner-temp")
                .arg(temp)
                .env("GITHUB_EVENT_NAME", "merge_group")
                .env("GITHUB_REF", event_ref)
                .env("GITHUB_REF_PROTECTED", "false")
                .env(
                    "GITHUB_WORKFLOW_REF",
                    "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
                ),
            deadline,
        )?;
        reject(&output, temp, expected)?;
    }
    Ok(())
}

pub(super) fn missing(binary: &str, root: &Path, temp: &Path, deadline: Instant) -> Result<()> {
    reject(
        &invoke(binary, root, temp, deadline)?,
        temp,
        "native_service_descriptor",
    )
}

pub(super) fn verify(binary: &str, root: &Path, temp: &Path, deadline: Instant) -> Result<()> {
    let path = temp.join("velnor-qualification-service.json");
    let original = fs::read(&path)?;
    for bytes in [b"{".as_slice(), b"{}".as_slice()] {
        descriptor_case(
            binary,
            root,
            temp,
            deadline,
            bytes,
            "native_service_descriptor",
        )?;
    }
    for (field, replacement, expected) in [
        (
            "container_id",
            serde_json::json!("0".repeat(64)),
            "native_service_command",
        ),
        (
            "daemon_id",
            serde_json::json!("invalid-daemon-id"),
            "native_service_daemon_identity",
        ),
        (
            "container_pid",
            serde_json::json!(0),
            "native_service_container",
        ),
        (
            "container_started_at",
            serde_json::json!("invalid-start"),
            "native_service_container",
        ),
        (
            "archive_sha256",
            serde_json::json!("0".repeat(64)),
            "native_service_archive_identity",
        ),
        (
            "unexpected",
            serde_json::json!(true),
            "native_service_descriptor",
        ),
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original)?;
        value[field] = replacement;
        descriptor_case(
            binary,
            root,
            temp,
            deadline,
            &serde_json::to_vec(&value)?,
            expected,
        )?;
    }
    unsafe_mode(binary, root, temp, deadline)?;
    descriptor_symlink(binary, root, temp, deadline)?;
    corrupt_archive(binary, root, temp, deadline)
}

fn descriptor_case(
    binary: &str,
    root: &Path,
    temp: &Path,
    deadline: Instant,
    replacement: &[u8],
    expected: &str,
) -> Result<()> {
    let path = temp.join("velnor-qualification-service.json");
    let original = fs::read(&path)?;
    fs::write(&path, replacement)?;
    let result = invoke(binary, root, temp, deadline);
    fs::write(&path, original)?;
    reject(&result?, temp, expected)
}

fn unsafe_mode(binary: &str, root: &Path, temp: &Path, deadline: Instant) -> Result<()> {
    let path = temp.join("velnor-qualification-service.json");
    let original = fs::metadata(&path)?.permissions();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    let result = invoke(binary, root, temp, deadline);
    fs::set_permissions(path, original)?;
    reject(&result?, temp, "native_service_descriptor")
}

fn descriptor_symlink(binary: &str, root: &Path, temp: &Path, deadline: Instant) -> Result<()> {
    let path = temp.join("velnor-qualification-service.json");
    let backup = temp.join("velnor-qualification-service.backup");
    fs::rename(&path, &backup)?;
    symlink(&backup, &path)?;
    let result = invoke(binary, root, temp, deadline);
    fs::remove_file(&path)?;
    fs::rename(&backup, &path)?;
    reject(&result?, temp, "native_service_descriptor")
}

fn corrupt_archive(binary: &str, root: &Path, temp: &Path, deadline: Instant) -> Result<()> {
    let archive = temp.join("velnor-qualification-service/docker.tgz");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(archive)?;
    let mut first = [0];
    file.read_exact(&mut first)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&[first[0] ^ 1])?;
    file.flush()?;
    let result = invoke(binary, root, temp, deadline);
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&first)?;
    file.flush()?;
    reject(&result?, temp, "native_service_archive_identity")
}

fn invoke(
    binary: &str,
    root: &Path,
    temp: &Path,
    deadline: Instant,
) -> Result<BoundedCommandOutput> {
    record_absent(temp)?;
    super::bounded(
        Command::new(binary)
            .args(["image-maintenance", "run-native-qualification", "--root"])
            .arg(root)
            .arg("--runner-temp")
            .arg(temp),
        deadline,
    )
}

fn reject(output: &BoundedCommandOutput, temp: &Path, expected: &str) -> Result<()> {
    assert_ne!(output.status, BoundedCommandStatus::Exited(0));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    record_absent(temp)
}

fn record_absent(temp: &Path) -> Result<()> {
    match fs::symlink_metadata(temp.join("native-image-qualification.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("qualification record exists during negative case".into()),
        Err(error) => Err(error.into()),
    }
}
