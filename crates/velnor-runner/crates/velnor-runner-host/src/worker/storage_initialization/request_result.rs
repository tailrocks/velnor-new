use serde::{Deserialize, Serialize};

use crate::error::HostError;

const INPUT_LIMIT: usize = 256;
const OUTPUT_LIMIT: usize = 2_048;
const HOME_TARGET: &str = "/velnor/init/home";
const WORK_TARGET: &str = "/velnor/init/work";

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageInitializationRequest {
    operation_id: String,
}

impl StorageInitializationRequest {
    pub(crate) fn new(operation_id: &str) -> Result<Self, HostError> {
        if !valid_operation_id(operation_id) {
            return Err(HostError::Identity);
        }
        Ok(Self {
            operation_id: operation_id.to_owned(),
        })
    }

    /// Write these bytes once. Send EOF after the last byte.
    pub(crate) fn stdin_payload(&self) -> Result<Vec<u8>, HostError> {
        let bytes = serde_json::to_vec(self).map_err(|_| HostError::Identity)?;
        if bytes.is_empty() || bytes.len() > INPUT_LIMIT {
            return Err(HostError::Identity);
        }
        Ok(bytes)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResultRecord {
    schema: u8,
    operation_id: String,
    home: DirectoryRecord,
    work: DirectoryRecord,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectoryRecord {
    target: String,
    mount_id: u64,
    parent_mount_id: u64,
    device_major: u32,
    device_minor: u32,
    inode: u64,
    uid: u32,
    gid: u32,
    mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedStorageInitialization {
    operation_id: String,
    home: ObservedDirectory,
    work: ObservedDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedDirectory {
    mount_id: u64,
    parent_mount_id: u64,
    device_major: u32,
    device_minor: u32,
    inode: u64,
}

impl ObservedStorageInitialization {
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) const fn home(&self) -> &ObservedDirectory {
        &self.home
    }

    pub(crate) const fn work(&self) -> &ObservedDirectory {
        &self.work
    }
}

impl ObservedDirectory {
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn parent_mount_id(&self) -> u64 {
        self.parent_mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn inode(&self) -> u64 {
        self.inode
    }
}

pub(crate) fn parse_result(
    exit_code: Option<i64>,
    expected_operation_id: &str,
    output: &[u8],
) -> Result<ObservedStorageInitialization, HostError> {
    if exit_code != Some(0)
        || !valid_operation_id(expected_operation_id)
        || output.is_empty()
        || output.len() > OUTPUT_LIMIT
        || output.last() != Some(&b'\n')
        || output[..output.len() - 1].contains(&b'\n')
    {
        return Err(HostError::Identity);
    }
    let record: ResultRecord =
        serde_json::from_slice(&output[..output.len() - 1]).map_err(|_| HostError::Identity)?;
    if serde_json::to_vec(&record).map_err(|_| HostError::Identity)? != output[..output.len() - 1] {
        return Err(HostError::Identity);
    }
    if record.schema != 1 || record.operation_id != expected_operation_id {
        return Err(HostError::Identity);
    }
    validate_directory(&record.home, HOME_TARGET)?;
    validate_directory(&record.work, WORK_TARGET)?;
    validate_separate_roots(&record.home, &record.work)?;
    Ok(ObservedStorageInitialization {
        operation_id: record.operation_id,
        home: record.home.observed(),
        work: record.work.observed(),
    })
}

impl DirectoryRecord {
    fn observed(&self) -> ObservedDirectory {
        ObservedDirectory {
            mount_id: self.mount_id,
            parent_mount_id: self.parent_mount_id,
            device_major: self.device_major,
            device_minor: self.device_minor,
            inode: self.inode,
        }
    }
}

fn validate_directory(record: &DirectoryRecord, target: &str) -> Result<(), HostError> {
    if record.target != target
        || record.mount_id == 0
        || record.parent_mount_id == 0
        || record.mount_id == record.parent_mount_id
        || record.inode == 0
        || record.uid != 1000
        || record.gid != 1000
        || record.mode != 0o755
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_separate_roots(
    home: &DirectoryRecord,
    work: &DirectoryRecord,
) -> Result<(), HostError> {
    let same_device =
        home.device_major == work.device_major && home.device_minor == work.device_minor;
    if home.mount_id == work.mount_id || (same_device && home.inode == work.inode) {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
