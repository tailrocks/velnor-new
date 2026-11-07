// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{build::support, validate};
use crate::{OrchestratorError, internal::internal};
use std::{fs, os::unix::fs::MetadataExt, path::Path};
use velnor_actions_contract::{
    QUALIFICATION_DAEMON_IMAGE, QUALIFICATION_DAEMON_SCRIPT, QualificationDaemonDescriptor,
};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

pub(super) fn verify(
    root: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let descriptor_path = temp.join("velnor-qualification-service.json");
    let metadata = fs::symlink_metadata(&descriptor_path)
        .map_err(|_| internal("native_service_descriptor"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(internal("native_service_descriptor"));
    }
    let bytes = validate::read_limited(&descriptor_path, 8192)?;
    let value = velnor_actions_contract::parse_strict_json(
        std::str::from_utf8(&bytes).map_err(|_| internal("native_service_descriptor"))?,
    )
    .map_err(|_| internal("native_service_descriptor"))?;
    let descriptor: QualificationDaemonDescriptor =
        serde_json::from_value(value).map_err(|_| internal("native_service_descriptor"))?;
    validate_descriptor(root, temp, &descriptor)?;
    verify_container(&descriptor, temp, deadline)?;
    let daemon = docker_inner(
        &descriptor.container_id,
        &["info", "--format", "{{.ID}}"],
        deadline,
    )?;
    if daemon.trim() != descriptor.daemon_id || descriptor.daemon_id.is_empty() {
        return Err(internal("native_service_daemon_identity"));
    }
    let lock = validate::read_lock(root)?;
    let version = docker_inner(
        &descriptor.container_id,
        &["version", "--format", "{{.Server.Version}}"],
        deadline,
    )?;
    if version.trim() != lock.docker_version {
        return Err(internal("native_service_daemon_version"));
    }
    Ok(descriptor.container_id)
}

fn validate_descriptor(
    root: &Path,
    temp: &Path,
    descriptor: &QualificationDaemonDescriptor,
) -> Result<(), OrchestratorError> {
    if descriptor.container_id.len() != 64
        || !descriptor
            .container_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(internal("native_service_container_identity"));
    }
    let archive = temp.join("velnor-qualification-service/docker.tgz");
    if archive.to_str() != Some(descriptor.archive_path.as_str()) {
        return Err(internal("native_service_archive_identity"));
    }
    let lock = validate::read_limited(&root.join(".velnor/runner.lock"), 1024 * 1024)?;
    let lock: toml::Value =
        toml::from_str(std::str::from_utf8(&lock).map_err(|_| internal("native_service_lock"))?)
            .map_err(|_| internal("native_service_lock"))?;
    let expected = lock
        .get("docker_sha256")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| internal("native_service_lock"))?;
    if descriptor.archive_sha256 != expected
        || support::hash_regular(&archive, 192 * 1024 * 1024)? != expected
    {
        return Err(internal("native_service_archive_identity"));
    }
    Ok(())
}

fn verify_container(
    descriptor: &QualificationDaemonDescriptor,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let host = Path::new("/var/run/docker.sock");
    let inspected = docker(host, &["inspect", &descriptor.container_id], deadline)?;
    let value = velnor_actions_contract::parse_strict_json(&inspected)
        .map_err(|_| internal("native_service_container"))?;
    let rows = value
        .as_array()
        .filter(|rows| rows.len() == 1)
        .ok_or_else(|| internal("native_service_container"))?;
    let item = &rows[0];
    let gid = rustix::process::getegid().as_raw().to_string();
    let expected_command = serde_json::json!([
        "-c",
        QUALIFICATION_DAEMON_SCRIPT,
        "velnor-q",
        descriptor.archive_sha256,
        gid
    ]);
    if item["Id"].as_str() != Some(&descriptor.container_id)
        || item["State"]["Running"].as_bool() != Some(true)
        || item["State"]["StartedAt"].as_str() != Some(&descriptor.container_started_at)
        || item["State"]["Pid"].as_u64() != Some(descriptor.container_pid)
        || descriptor.container_pid == 0
        || descriptor.container_started_at.is_empty()
        || item["Config"]["Image"].as_str() != Some(QUALIFICATION_DAEMON_IMAGE)
        || item["Config"]["Entrypoint"] != serde_json::json!(["/bin/sh"])
        || item["Config"]["Cmd"] != expected_command
        || item["Config"]["OpenStdin"].as_bool() != Some(true)
        || item["HostConfig"]["Privileged"].as_bool() != Some(true)
        || item["HostConfig"]["NetworkMode"].as_str() != Some("none")
    {
        return Err(internal("native_service_container"));
    }
    let image = docker(
        host,
        &["image", "inspect", QUALIFICATION_DAEMON_IMAGE],
        deadline,
    )?;
    let image = velnor_actions_contract::parse_strict_json(&image)
        .map_err(|_| internal("native_service_image"))?;
    let rows = image
        .as_array()
        .filter(|rows| rows.len() == 1)
        .ok_or_else(|| internal("native_service_image"))?;
    if item["Image"] != rows[0]["Id"] {
        return Err(internal("native_service_image"));
    }
    verify_mounts(item, descriptor, temp)
}

fn verify_mounts(
    item: &serde_json::Value,
    descriptor: &QualificationDaemonDescriptor,
    temp: &Path,
) -> Result<(), OrchestratorError> {
    let mounts = item["Mounts"]
        .as_array()
        .filter(|mounts| mounts.len() == 2)
        .ok_or_else(|| internal("native_service_mounts"))?;
    for (source, destination, writable) in [
        (descriptor.archive_path.as_str(), "/docker.tgz", false),
        (
            temp.to_str()
                .ok_or_else(|| internal("native_service_mounts"))?,
            temp.to_str()
                .ok_or_else(|| internal("native_service_mounts"))?,
            true,
        ),
    ] {
        if mounts
            .iter()
            .filter(|mount| {
                mount["Type"].as_str() == Some("bind")
                    && mount["Source"].as_str() == Some(source)
                    && mount["Destination"].as_str() == Some(destination)
                    && mount["RW"].as_bool() == Some(writable)
            })
            .count()
            != 1
        {
            return Err(internal("native_service_mounts"));
        }
    }
    Ok(())
}

fn docker(
    socket: &Path,
    args: &[&str],
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let command = IsolatedCommand::repo_task(
        "/usr/bin/docker",
        args.iter().map(Into::into).collect(),
        &[],
    )
    .and_then(|command| command.with_private_docker_socket(socket))
    .map_err(|_| internal("native_service_command"))?;
    let (output, _) = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| internal("native_service_command"))?;
    output
        .require_success("docker")
        .map_err(|_| internal("native_service_command"))?;
    output
        .stdout_text("docker")
        .map_err(|_| internal("native_service_output"))
}

fn docker_inner(
    container_id: &str,
    args: &[&str],
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let command = IsolatedCommand::repo_task(
        "/usr/bin/docker",
        args.iter().map(Into::into).collect(),
        &[],
    )
    .and_then(|command| command.in_qualification_container(container_id))
    .map_err(|_| internal("native_service_command"))?;
    let (output, _) = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| internal("native_service_command"))?;
    output
        .require_success("docker")
        .map_err(|_| internal("native_service_command"))?;
    output
        .stdout_text("docker")
        .map_err(|_| internal("native_service_output"))
}
