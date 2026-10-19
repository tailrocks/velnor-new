use serde::{Deserialize, Serialize};

use crate::error::HostError;

const INPUT_LIMIT: usize = 256;
const OUTPUT_LIMIT: usize = 2_048;
const HOME_TARGET: &str = "/velnor/init/home";
const WORK_TARGET: &str = "/velnor/init/work";
const METADATA_TARGET: &str = "/velnor/init/metadata";
const RUNTIME_IDENTITY_TARGET: &str = "/velnor/init/metadata/runtime-identity";
const JOB_FACTS_TARGET: &str = "/velnor/init/metadata/job-facts";
const SOURCE_PROOF_TARGET: &str = "/velnor/init/metadata/source-proof";
const CONTROL_TARGET: &str = "/velnor/init/metadata/source-proof/control";
const INBOX_TARGET: &str = "/velnor/init/home/source-proof/inbox";
const HEX: &[u8; 16] = b"0123456789abcdef";

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

    /// Write this bounded request line once and keep the stream open for the ACK.
    pub(crate) fn stdin_payload(&self) -> Result<Vec<u8>, HostError> {
        let mut bytes = serde_json::to_vec(self).map_err(|_| HostError::Identity)?;
        bytes.push(b'\n');
        if bytes.len() > INPUT_LIMIT {
            return Err(HostError::Identity);
        }
        Ok(bytes)
    }
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageInitializationAcknowledgment {
    operation_id: String,
}

impl StorageInitializationAcknowledgment {
    pub(crate) fn new(operation_id: &str) -> Result<Self, HostError> {
        if !valid_operation_id(operation_id) {
            return Err(HostError::Identity);
        }
        Ok(Self {
            operation_id: operation_id.to_owned(),
        })
    }

    pub(crate) fn stdin_payload(&self) -> Result<Vec<u8>, HostError> {
        let mut bytes = serde_json::to_vec(self).map_err(|_| HostError::Identity)?;
        bytes.push(b'\n');
        if bytes.len() > INPUT_LIMIT {
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
    metadata: DirectoryRecord,
    runtime_identity: DirectoryRecord,
    job_facts: DirectoryRecord,
    source_proof: DirectoryRecord,
    control: DirectoryRecord,
    inbox: DirectoryRecord,
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
    metadata: ObservedDirectory,
    runtime_identity: ObservedDirectory,
    job_facts: ObservedDirectory,
    source_proof: ObservedDirectory,
    control: ObservedDirectory,
    inbox: ObservedDirectory,
    layout_digest: String,
    ready_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedDirectory {
    mount_id: u64,
    parent_mount_id: u64,
    device_major: u32,
    device_minor: u32,
    inode: u64,
    uid: u32,
    gid: u32,
    mode: u32,
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

    #[cfg(test)]
    pub(crate) const fn metadata(&self) -> &ObservedDirectory {
        &self.metadata
    }

    #[cfg(test)]
    pub(crate) const fn runtime_identity(&self) -> &ObservedDirectory {
        &self.runtime_identity
    }

    pub(crate) const fn job_facts(&self) -> &ObservedDirectory {
        &self.job_facts
    }

    #[cfg(test)]
    pub(crate) const fn source_proof(&self) -> &ObservedDirectory {
        &self.source_proof
    }

    #[cfg(test)]
    pub(crate) const fn control(&self) -> &ObservedDirectory {
        &self.control
    }

    #[cfg(test)]
    pub(crate) const fn inbox(&self) -> &ObservedDirectory {
        &self.inbox
    }

    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }

    pub(crate) fn ready_digest(&self) -> &str {
        &self.ready_digest
    }
}

impl ObservedDirectory {
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    #[cfg(test)]
    pub(crate) const fn parent_mount_id(&self) -> u64 {
        self.parent_mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn inode(&self) -> u64 {
        self.inode
    }

    pub(crate) const fn owner(&self) -> (u32, u32) {
        (self.uid, self.gid)
    }

    pub(crate) const fn mode(&self) -> u32 {
        self.mode
    }
}

pub(crate) fn parse_ready(
    expected_operation_id: &str,
    output: &[u8],
) -> Result<ObservedStorageInitialization, HostError> {
    if !valid_operation_id(expected_operation_id)
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
    if record.schema != 2 || record.operation_id != expected_operation_id {
        return Err(HostError::Identity);
    }
    validate_mount_root(&record.home, HOME_TARGET, (1000, 1000, 0o755))?;
    validate_mount_root(&record.work, WORK_TARGET, (1000, 1000, 0o755))?;
    validate_mount_root(&record.metadata, METADATA_TARGET, (0, 0, 0o555))?;
    validate_child(
        &record.runtime_identity,
        RUNTIME_IDENTITY_TARGET,
        &record.metadata,
    )?;
    validate_child(&record.job_facts, JOB_FACTS_TARGET, &record.metadata)?;
    validate_child(&record.source_proof, SOURCE_PROOF_TARGET, &record.metadata)?;
    validate_child(&record.control, CONTROL_TARGET, &record.source_proof)?;
    validate_child(&record.inbox, INBOX_TARGET, &record.home)?;
    validate_separate_roots(&[&record.home, &record.work, &record.metadata])?;
    validate_separate_children(
        &record.metadata,
        &[
            &record.runtime_identity,
            &record.job_facts,
            &record.source_proof,
            &record.control,
        ],
    )?;
    validate_separate_children(&record.home, &[&record.inbox])?;
    let canonical = serde_json::to_vec(&record).map_err(|_| HostError::Identity)?;
    if canonical != output[..output.len() - 1] {
        return Err(HostError::Identity);
    }
    let layout_digest = sha256_digest(&canonical);
    Ok(ObservedStorageInitialization {
        operation_id: record.operation_id,
        home: record.home.observed(),
        work: record.work.observed(),
        metadata: record.metadata.observed(),
        runtime_identity: record.runtime_identity.observed(),
        job_facts: record.job_facts.observed(),
        source_proof: record.source_proof.observed(),
        control: record.control.observed(),
        inbox: record.inbox.observed(),
        layout_digest,
        ready_digest: sha256_digest(output),
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
            uid: self.uid,
            gid: self.gid,
            mode: self.mode,
        }
    }
}

fn validate_mount_root(
    record: &DirectoryRecord,
    target: &str,
    owner_mode: (u32, u32, u32),
) -> Result<(), HostError> {
    if record.target != target
        || record.mount_id == 0
        || record.parent_mount_id == 0
        || record.mount_id == record.parent_mount_id
        || record.inode == 0
        || (record.uid, record.gid, record.mode) != owner_mode
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_child(
    record: &DirectoryRecord,
    target: &str,
    parent: &DirectoryRecord,
) -> Result<(), HostError> {
    if record.target != target
        || record.mount_id != parent.mount_id
        || record.parent_mount_id != parent.mount_id
        || record.device_major != parent.device_major
        || record.device_minor != parent.device_minor
        || record.inode == 0
    {
        return Err(HostError::Identity);
    }
    let expected = match target {
        RUNTIME_IDENTITY_TARGET | JOB_FACTS_TARGET | CONTROL_TARGET => (0, 0, 0o555),
        SOURCE_PROOF_TARGET => (0, 0, 0o755),
        INBOX_TARGET => (1000, 1000, 0o700),
        _ => return Err(HostError::Identity),
    };
    if (record.uid, record.gid, record.mode) != expected {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_separate_roots(roots: &[&DirectoryRecord]) -> Result<(), HostError> {
    for (index, left) in roots.iter().enumerate() {
        for right in roots.iter().skip(index + 1) {
            let same_device =
                left.device_major == right.device_major && left.device_minor == right.device_minor;
            if left.mount_id == right.mount_id || (same_device && left.inode == right.inode) {
                return Err(HostError::Identity);
            }
        }
    }
    Ok(())
}

fn validate_separate_children(
    metadata: &DirectoryRecord,
    children: &[&DirectoryRecord],
) -> Result<(), HostError> {
    if children.iter().any(|child| {
        child.inode == metadata.inode
            || child.device_major != metadata.device_major
            || child.device_minor != metadata.device_minor
            || child.mount_id != metadata.mount_id
    }) {
        return Err(HostError::Identity);
    }
    for (index, left) in children.iter().enumerate() {
        for right in children.iter().skip(index + 1) {
            if left.inode == right.inode
                || left.device_major != right.device_major
                || left.device_minor != right.device_minor
                || left.mount_id != right.mount_id
            {
                return Err(HostError::Identity);
            }
        }
    }
    Ok(())
}

fn sha256_digest(value: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
