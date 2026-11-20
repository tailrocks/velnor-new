// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Instant;

use rustix::fs::{self, AtFlags, Mode, OFlags, RenameFlags};
use velnor_runner_image_manifest::{RunnerImageQualification, validate_runner_image_qualification};

use super::source::{self, SourceSnapshot};
use crate::error::HostError;

pub(super) async fn publish(
    output: &Path,
    source: &SourceSnapshot,
    proof: &RunnerImageQualification,
    end: Instant,
) -> Result<(), HostError> {
    validate_runner_image_qualification(proof).map_err(|_| HostError::Identity)?;
    if !output.is_absolute() || output.starts_with(&source.root) {
        return Err(HostError::Path);
    }
    let parent_path = output.parent().ok_or(HostError::Path)?;
    if parent_path.canonicalize().map_err(|_| HostError::Path)? != parent_path
        || parent_path.starts_with(&source.root)
    {
        return Err(HostError::Path);
    }
    let name = output.file_name().ok_or(HostError::Path)?;
    let parent = std::fs::File::from(
        fs::open(
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?,
    );
    let parent_metadata = parent.metadata().map_err(|_| HostError::Path)?;
    if parent_metadata.uid() != rustix::process::geteuid().as_raw()
        || parent_metadata.mode() & 0o077 != 0
    {
        return Err(HostError::Ownership);
    }
    let stage_name = format!(".velnor-qualification-{}", uuid::Uuid::new_v4());
    let mut file = std::fs::File::from(
        fs::openat(
            &parent,
            stage_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| HostError::Path)?,
    );
    let result = async {
        let mut bytes = serde_json::to_vec(proof).map_err(|_| HostError::Identity)?;
        bytes.push(b'\n');
        file.write_all(&bytes).map_err(|_| HostError::Path)?;
        file.sync_all().map_err(|_| HostError::Path)?;
        super::deadline(source::revalidate(source), super::remaining(end)?).await??;
        let held = fs::fstat(&file).map_err(|_| HostError::Path)?;
        let stage = fs::statat(&parent, stage_name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| HostError::Path)?;
        let named_parent = std::fs::symlink_metadata(parent_path).map_err(|_| HostError::Path)?;
        if held.st_dev != stage.st_dev
            || held.st_ino != stage.st_ino
            || parent_metadata.dev() != named_parent.dev()
            || parent_metadata.ino() != named_parent.ino()
        {
            return Err(HostError::Ownership);
        }
        super::remaining(end)?;
        fs::renameat_with(
            &parent,
            stage_name.as_str(),
            &parent,
            name,
            RenameFlags::NOREPLACE,
        )
        .map_err(|_| HostError::Path)?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let held = fs::fstat(&file).map_err(|_| HostError::Ownership)?;
        let stage = fs::statat(&parent, stage_name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| HostError::Ownership)?;
        if held.st_dev != stage.st_dev || held.st_ino != stage.st_ino {
            return Err(HostError::Ownership);
        }
        fs::unlinkat(&parent, stage_name.as_str(), AtFlags::empty())
            .map_err(|_| HostError::Ownership)?;
    }
    result
}
