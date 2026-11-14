// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Digest, HostError, ImageKind, Instant, MetadataExt, Path, PathBuf,
    RunnerImagePublicationRecord, SOURCE_LIMIT, Sha256, SnapshotDirectory, SourceSnapshot,
    runner_publication_source_inputs,
};

#[derive(serde::Deserialize)]
struct BaseLock {
    #[serde(rename = "base_image")]
    image: String,
    #[serde(rename = "base_image_name")]
    name: String,
    #[serde(rename = "base_image_digest")]
    digest: String,
    #[serde(rename = "base_image_platform")]
    platform: String,
}

pub(super) fn base_image(bytes: &[u8]) -> Result<String, HostError> {
    let lock: BaseLock =
        toml::from_str(std::str::from_utf8(bytes).map_err(|_| HostError::Identity)?)
            .map_err(|_| HostError::Identity)?;
    let digest = lock
        .digest
        .strip_prefix("sha256:")
        .ok_or(HostError::Identity)?;
    if lock.name != "ubuntu:26.04"
        || lock.platform != "linux/amd64"
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || lock.image != format!("{}@{}", lock.name, lock.digest)
    {
        return Err(HostError::Identity);
    }
    Ok(lock.image)
}

pub(super) fn validate_records(
    source: &SourceSnapshot,
    records: [&Option<RunnerImagePublicationRecord>; 3],
    end: Instant,
) -> Result<(), HostError> {
    let [runner, dind, probe] = records;
    let (Some(runner), Some(dind), Some(probe)) = (runner, dind, probe) else {
        return if runner.is_none() && dind.is_none() && probe.is_none() {
            Ok(())
        } else {
            Err(HostError::Identity)
        };
    };
    if runner.source.commit != source.source_sha
        || runner.lock.sha256 != sha256_hex(&source.lock_bytes)
        || runner.lock.runner_version != source.runner_version
        || runner.lock.docker_version != source.docker_version
        || runner.lock.buildx_version != source.buildx_version
        || runner.lock.compose_version != source.compose_version
        || runner.lock.node_version != source.node_version
        || [dind, probe].iter().any(|record| {
            record.source != runner.source
                || record.lock != runner.lock
                || record.workflow != runner.workflow
        })
    {
        return Err(HostError::Identity);
    }
    for &path in runner_publication_source_inputs().map_err(|_| HostError::Identity)? {
        if Instant::now() >= end {
            return Err(HostError::DockerTimeout);
        }
        let bytes = super::super::super::files::read_bounded_regular(
            &source.root.join(path),
            SOURCE_LIMIT,
        )?;
        if runner.source.inputs.get(path) != Some(&sha256_hex(&bytes)) {
            return Err(HostError::Identity);
        }
    }
    if Instant::now() >= end {
        return Err(HostError::DockerTimeout);
    }
    Ok(())
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

pub(super) fn validate_held_file(file: &std::fs::File, path: &Path) -> Result<(), HostError> {
    let held = file.metadata().map_err(|_| HostError::Path)?;
    let named = std::fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if !held.is_file()
        || !named.is_file()
        || held.dev() != named.dev()
        || held.ino() != named.ino()
        || held.len() != named.len()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn role_name(kind: ImageKind) -> &'static str {
    match kind {
        ImageKind::Runner => "runner",
        ImageKind::Dind => "dind",
        ImageKind::ResourceProbe => "resource_probe",
    }
}

pub(super) fn snapshot_path(
    directory: &SnapshotDirectory,
    name: &str,
) -> Result<PathBuf, HostError> {
    Ok(directory
        .path
        .as_ref()
        .ok_or(HostError::Ownership)?
        .path()
        .join(name))
}
