// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};
use velnor_runner_image_manifest::RunnerImageNamedContext;

use super::snapshot::{SnapshotDirectory, create_bounded_snapshot};
use crate::error::HostError;

const NAMES: [&str; 4] = [
    "resource_probe",
    "storage_init",
    "job_start_hook",
    "job_started_hook",
];
const LIMIT: u64 = 1024 * 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) struct Contexts {
    files: BTreeMap<String, Context>,
}

struct Context {
    file: std::fs::File,
    path: PathBuf,
    evidence: RunnerImageNamedContext,
}

impl Contexts {
    pub(super) fn capture(
        root: &Path,
        directory: &mut SnapshotDirectory,
        end: Instant,
    ) -> Result<Self, HostError> {
        let mut files = BTreeMap::new();
        for name in NAMES {
            let snapshot_name = format!("context-{name}");
            let file =
                create_bounded_snapshot(&root.join(name), directory, &snapshot_name, end, LIMIT)?;
            let path = directory
                .path
                .as_ref()
                .ok_or(HostError::Ownership)?
                .path()
                .join(snapshot_name);
            let (sha256, size_bytes) = digest(&file, &path, end)?;
            files.insert(
                name.to_owned(),
                Context {
                    file,
                    path,
                    evidence: RunnerImageNamedContext {
                        path: format!("target/velnor-runner-family/{name}"),
                        sha256,
                        size_bytes,
                    },
                },
            );
        }
        Ok(Self { files })
    }

    pub(super) fn expected(&self, runner: bool) -> BTreeMap<String, RunnerImageNamedContext> {
        self.files
            .iter()
            .filter(|(name, _)| runner || name.as_str() == "resource_probe")
            .map(|(name, context)| (name.clone(), context.evidence.clone()))
            .collect()
    }

    pub(super) fn revalidate(&self, end: Instant) -> Result<(), HostError> {
        for context in self.files.values() {
            let (sha256, size_bytes) = digest(&context.file, &context.path, end)?;
            if sha256 != context.evidence.sha256 || size_bytes != context.evidence.size_bytes {
                return Err(HostError::Identity);
            }
        }
        Ok(())
    }
}

fn digest(file: &std::fs::File, path: &Path, end: Instant) -> Result<(String, u64), HostError> {
    let held = file.metadata().map_err(|_| HostError::Path)?;
    let named = std::fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if !held.is_file()
        || !named.is_file()
        || held.dev() != named.dev()
        || held.ino() != named.ino()
        || held.len() == 0
        || held.len() > LIMIT
        || held.len() != named.len()
    {
        return Err(HostError::Identity);
    }
    let mut input = file.try_clone().map_err(|_| HostError::Path)?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Path)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 65536];
    loop {
        if Instant::now() >= end {
            return Err(HostError::DockerTimeout);
        }
        let count = input.read(&mut buffer).map_err(|_| HostError::Path)?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count as u64).ok_or(HostError::Frame)?;
        if total > LIMIT {
            return Err(HostError::Frame);
        }
        hasher.update(&buffer[..count]);
    }
    if Instant::now() >= end {
        return Err(HostError::DockerTimeout);
    }
    if total != held.len() || file.metadata().map_err(|_| HostError::Path)?.len() != total {
        return Err(HostError::Identity);
    }
    let mut result = String::with_capacity(64);
    for byte in hasher.finalize() {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok((result, total))
}
