// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path},
    time::Instant,
};

use rustix::fs::{Mode, OFlags, open, openat};
use serde::{Deserialize, Serialize};

use crate::ProbeError;

use super::{
    DECISION_FILE, DECISION_LIMIT, DECISION_POLL_INTERVAL, Decision, REQUEST_FILE, RUNNER_UID,
    validation,
};

pub(super) fn open_trusted_directory(
    path: &str,
    owner: u32,
    private_owner_only: bool,
) -> Result<File, ProbeError> {
    let path = Path::new(path);
    let mut components = Vec::new();
    for (index, component) in path.components().enumerate() {
        match (index, component) {
            (0, Component::RootDir) => {}
            (_, Component::Normal(name)) => {
                components.push(name.to_str().ok_or(ProbeError::Invalid("job_hook_path"))?);
            }
            _ => return Err(ProbeError::Invalid("job_hook_path")),
        }
    }
    if !path.is_absolute() || components.is_empty() {
        return Err(ProbeError::Invalid("job_hook_path"));
    }
    let mut current = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("job_hook_directory"))?,
    );
    validate_directory_owner(&current, 0, false)?;
    for (index, component) in components.iter().enumerate() {
        let next = openat(
            &current,
            *component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("job_hook_directory"))?;
        current = File::from(next);
        let final_component = index + 1 == components.len();
        validate_directory_owner(
            &current,
            if final_component { owner } else { 0 },
            final_component && private_owner_only,
        )?;
    }
    Ok(current)
}

fn validate_directory_owner(
    directory: &File,
    owner: u32,
    private_owner_only: bool,
) -> Result<(), ProbeError> {
    let metadata = directory
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_hook_directory"))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_dir()
        || metadata.uid() != owner
        || mode & 0o022 != 0
        || (private_owner_only && mode & 0o077 != 0)
        || (!private_owner_only && mode & 0o005 != 0o005)
    {
        return Err(ProbeError::Invalid("job_hook_directory_owner"));
    }
    Ok(())
}

pub(super) fn read_json_at<T: for<'de> Deserialize<'de> + Serialize>(
    directory: &File,
    name: &str,
    owner: u32,
    limit: usize,
    readonly: bool,
) -> Result<T, ProbeError> {
    let file = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_hook_file"))?;
    let file = File::from(file);
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_hook_file"))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_file()
        || metadata.uid() != owner
        || (readonly && mode & 0o222 != 0)
        || (readonly && mode & 0o004 == 0)
        || (!readonly && mode & 0o077 != 0)
        || metadata.len() > limit as u64
    {
        return Err(ProbeError::Invalid("job_hook_file_owner"));
    }
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_hook_file_read"))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge("job_hook_file"));
    }
    let value: T =
        serde_json::from_slice(&bytes).map_err(|_| ProbeError::Invalid("job_hook_json"))?;
    let canonical = validation::canonical_json(&value, limit, "job_hook_json")?;
    if canonical != bytes {
        return Err(ProbeError::Invalid("job_hook_json_canonical"));
    }
    Ok(value)
}

pub(super) fn write_request_at(
    inbox: &File,
    inbox_path: &Path,
    bytes: &[u8],
    challenge: &str,
) -> Result<(), ProbeError> {
    let temporary = format!("request.{challenge}.tmp");
    let temp_fd = openat(
        inbox,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| ProbeError::Invalid("job_hook_request_replay"))?;
    let mut temp_file = File::from(temp_fd);
    temp_file
        .write_all(bytes)
        .and_then(|()| temp_file.sync_all())
        .map_err(|_| ProbeError::Invalid("job_hook_request_write"))?;
    let temp_path = inbox_path.join(&temporary);
    let request_path = inbox_path.join(REQUEST_FILE);
    fs::hard_link(&temp_path, &request_path)
        .map_err(|_| ProbeError::Invalid("job_hook_request_replay"))?;
    inbox
        .sync_all()
        .map_err(|_| ProbeError::Invalid("job_hook_request_publish"))?;
    fs::remove_file(&temp_path).map_err(|_| ProbeError::Invalid("job_hook_request_publish"))?;
    Ok(())
}

pub(super) fn wait_for_decision(
    control: &File,
    control_path: &Path,
    deadline: Instant,
) -> Result<Decision, ProbeError> {
    loop {
        if Instant::now() >= deadline {
            return Err(ProbeError::Invalid("job_hook_decision_timeout"));
        }
        match read_json_at::<Decision>(control, DECISION_FILE, 0, DECISION_LIMIT, true) {
            Ok(decision) => return Ok(decision),
            Err(ProbeError::Invalid("job_hook_file"))
                if !control_path.join(DECISION_FILE).exists() =>
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                std::thread::sleep(remaining.min(DECISION_POLL_INTERVAL));
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn read_bounded_path(path: &Path, limit: usize) -> Result<Vec<u8>, ProbeError> {
    if !path.is_absolute() {
        return Err(ProbeError::Invalid("job_hook_event_path"));
    }
    let fd = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_hook_event_path"))?;
    let file = File::from(fd);
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_hook_event_file"))?;
    if !metadata.is_file()
        || metadata.uid() != RUNNER_UID
        || metadata.mode() & 0o022 != 0
        || metadata.len() > limit as u64
    {
        return Err(ProbeError::Invalid("job_hook_event_file"));
    }
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_hook_event_read"))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge("job_hook_event"));
    }
    Ok(bytes)
}
