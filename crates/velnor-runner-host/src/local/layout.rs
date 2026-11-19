// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::{Mount, MountType, MountVolumeOptions};

use crate::error::HostError;

pub(super) const METADATA_PARENT: &str = "/velnor/init";
pub(super) const METADATA_TARGET: &str = "/velnor/init/metadata";
pub(super) const HOME_TARGET: &str = "/home/runner";
pub(super) const RUNNER_OWNED_ROOT: &str = ",uid=1000,gid=1000,mode=0755";

const METADATA_DIRECTORY: &str = "metadata";
const RUNNER_ID: u64 = 1000;
const ROOT_ID: u64 = 0;

const METADATA: [(&str, u64, u32); 5] = [
    ("metadata/", ROOT_ID, 0o555),
    ("metadata/runtime-identity/", ROOT_ID, 0o555),
    ("metadata/job-facts/", ROOT_ID, 0o555),
    ("metadata/source-proof/", ROOT_ID, 0o755),
    ("metadata/source-proof/control/", ROOT_ID, 0o555),
];

const HOME: [(&str, u64, u32); 2] = [
    ("source-proof/", ROOT_ID, 0o755),
    ("source-proof/inbox/", RUNNER_ID, 0o700),
];

pub(super) fn metadata_archive() -> Result<Vec<u8>, HostError> {
    directories(&METADATA)
}

pub(super) fn home_archive() -> Result<Vec<u8>, HostError> {
    directories(&HOME)
}

fn directories(entries: &[(&str, u64, u32)]) -> Result<Vec<u8>, HostError> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, owner, mode) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_path(path).map_err(|_| HostError::Identity)?;
        header.set_uid(*owner);
        header.set_gid(*owner);
        header.set_mode(*mode);
        header.set_size(0);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append(&header, std::io::empty())
            .map_err(|_| HostError::Identity)?;
    }
    builder.into_inner().map_err(|_| HostError::Identity)
}

const LOCAL_CI_SUBPATH: &str = "local-ci";
pub(super) const LOCAL_CI_TARGET: &str = "/opt/velnor/local";
const LOCAL_CI_BINARY: &str = "velnor-ci";
const LOCAL_CI_MODE: u32 = 0o555;

/// The local velnor-ci as `metadata/local-ci/velnor-ci`, root-owned and read-only,
/// to be unpacked into the metadata volume beside the directories of `metadata_archive`.
pub(super) fn local_ci_archive(binary: &[u8]) -> Result<Vec<u8>, HostError> {
    let directory = format!("{METADATA_DIRECTORY}/{LOCAL_CI_SUBPATH}");
    let mut builder = tar::Builder::new(Vec::new());
    for (path, kind, size) in [
        (format!("{directory}/"), tar::EntryType::Directory, 0),
        (
            format!("{directory}/{LOCAL_CI_BINARY}"),
            tar::EntryType::Regular,
            u64::try_from(binary.len()).map_err(|_| HostError::Identity)?,
        ),
    ] {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(kind);
        header.set_path(path).map_err(|_| HostError::Identity)?;
        header.set_uid(ROOT_ID);
        header.set_gid(ROOT_ID);
        header.set_mode(LOCAL_CI_MODE);
        header.set_size(size);
        header.set_mtime(0);
        header.set_cksum();
        let body: &[u8] = if size == 0 { &[] } else { binary };
        builder
            .append(&header, body)
            .map_err(|_| HostError::Identity)?;
    }
    builder.into_inner().map_err(|_| HostError::Identity)
}

/// What the runner sees at `/opt/velnor/local`: only the `local-ci` subpath of the metadata volume.
pub(super) fn local_ci_mount(metadata_volume: String) -> Mount {
    Mount {
        target: Some(LOCAL_CI_TARGET.to_owned()),
        source: Some(metadata_volume),
        typ: Some(MountType::VOLUME),
        read_only: Some(true),
        volume_options: Some(MountVolumeOptions {
            no_copy: Some(true),
            subpath: Some(LOCAL_CI_SUBPATH.to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) const INBOX_REQUEST: &str = "/opt/velnor/source-proof/inbox/request.json";
const CONTROL_DIRECTORY: &str = "metadata/source-proof/control";
const CONTROL_FILE_MODE: u32 = 0o444;

pub(super) fn control_archive(name: &str, bytes: &[u8]) -> Result<Vec<u8>, HostError> {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header
        .set_path(format!("{CONTROL_DIRECTORY}/{name}"))
        .map_err(|_| HostError::Identity)?;
    header.set_uid(ROOT_ID);
    header.set_gid(ROOT_ID);
    header.set_mode(CONTROL_FILE_MODE);
    header.set_size(u64::try_from(bytes.len()).map_err(|_| HostError::Identity)?);
    header.set_mtime(0);
    header.set_cksum();
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append(&header, bytes)
        .map_err(|_| HostError::Identity)?;
    builder.into_inner().map_err(|_| HostError::Identity)
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
