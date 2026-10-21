use super::digest;
use crate::error::HostError;
use serde::{Deserialize, Serialize};
use velnor_resource_probe::execution_channel_wire::ExecutionChannelObservation;

const CONTROL_TARGET: &str = "/opt/velnor/source-proof/control";
const INBOX_TARGET: &str = "/opt/velnor/source-proof/inbox";
const JOB_FACTS_TARGET: &str = "/opt/velnor/job-facts";
const CONTROL_SUBPATH: &str = "source-proof/control";
const INBOX_SUBPATH: &str = "source-proof/inbox";
const JOB_FACTS_SUBPATH: &str = "job-facts";
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProofChannelRecord {
    schema_version: u32,
    reservation_id: i64,
    operation_id: i64,
    connection_id: String,
    scale_set_id: i64,
    witness_request_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_id: i64,
    runner_name: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    control: ProofChannelMountRecord,
    inbox: ProofChannelMountRecord,
    job_facts: ProofChannelMountRecord,
    observation: ExecutionChannelObservation,
    dind_attachment_absent: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProofChannelMountRecord {
    target: String,
    subpath: String,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    fs_type_magic: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
    directory_inode: u64,
    owner_uid: u32,
    owner_gid: u32,
    mode: u32,
    read_only: bool,
    no_copy: bool,
}

impl ProofChannelRecord {
    pub(crate) fn parse(json: &str, expected_digest: &str) -> Result<Self, HostError> {
        let value: Self = serde_json::from_str(json).map_err(|_| HostError::Journal)?;
        let canonical = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
        if canonical != json.as_bytes() || digest(&canonical) != expected_digest || !value.valid() {
            return Err(HostError::Journal);
        }
        Ok(value)
    }

    #[cfg(test)]
    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(self).map_err(|_| HostError::Journal)
    }

    pub(crate) fn valid(&self) -> bool {
        self.schema_version == 3
            && self.reservation_id > 0
            && self.operation_id > 0
            && valid_text(&self.connection_id, 128)
            && self.scale_set_id > 0
            && self.witness_request_id > 0
            && valid_text(&self.engine_id, 128)
            && valid_digest(&self.docker_root_digest)
            && self.runner_id > 0
            && valid_text(&self.runner_name, 128)
            && valid_object_id(&self.runner_container_id)
            && valid_object_id(&self.dind_container_id)
            && self.runner_container_id != self.dind_container_id
            && valid_digest(&self.instance_binding_digest)
            && valid_volume_name(&self.metadata_volume_name)
            && valid_object_id(&self.metadata_volume_id)
            && self.control.valid_control()
            && self.inbox.valid_inbox()
            && self.job_facts.valid_job_facts()
            && self.observation.schema_version == 3
            && valid_operation_id(&self.observation.operation_id)
            && !self.observation.guest_boot_id.is_empty()
            && self.observation.mount_namespace_id > 0
            && directory_matches_mount(&self.observation.control, &self.control)
            && directory_matches_mount(&self.observation.inbox, &self.inbox)
            && directory_matches_mount(&self.observation.job_facts, &self.job_facts)
            && self.control.mount_id != self.inbox.mount_id
            && self.job_facts.mount_id != self.inbox.mount_id
            && self.job_facts.mount_id != self.control.mount_id
            && self.control.directory_inode != self.inbox.directory_inode
            && self.job_facts.directory_inode != self.control.directory_inode
            && self.job_facts.directory_inode != self.inbox.directory_inode
            && self.dind_attachment_absent
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) const fn witness_request_id(&self) -> i64 {
        self.witness_request_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(crate) fn control(&self) -> &ProofChannelMountRecord {
        &self.control
    }

    #[cfg(test)]
    pub(crate) fn inbox(&self) -> &ProofChannelMountRecord {
        &self.inbox
    }
}

fn valid_operation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn directory_matches_mount(
    directory: &velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory,
    mount: &ProofChannelMountRecord,
) -> bool {
    directory.target == mount.target
        && directory.inode == mount.directory_inode
        && directory.owner_uid == mount.owner_uid
        && directory.owner_gid == mount.owner_gid
        && directory.mode == mount.mode
        && directory.mount_id == mount.mount_id
        && directory.mount_device_major == mount.device_major
        && directory.mount_device_minor == mount.device_minor
        && directory.statx_device_major == mount.statx_device_major
        && directory.statx_device_minor == mount.statx_device_minor
        && directory.filesystem_type_magic == mount.fs_type_magic
        && directory.filesystem_id == mount.filesystem_id
        && directory.read_only == mount.read_only
        && valid_mount_root(directory)
}

fn valid_mount_root(
    directory: &velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory,
) -> bool {
    let suffix = match directory.target.as_str() {
        CONTROL_TARGET => "/source-proof/control",
        INBOX_TARGET => "/source-proof/inbox",
        JOB_FACTS_TARGET => "/job-facts",
        _ => return false,
    };
    let root = directory.mount_root.as_str();
    root.starts_with('/')
        && root.ends_with(suffix)
        && root
            .split('/')
            .all(|component| component != "." && component != "..")
        && !root.chars().any(char::is_control)
}

#[derive(Clone, Copy)]
pub(crate) struct PublishedChannelDirectory<'a> {
    pub(crate) volume_name: &'a str,
    pub(crate) volume_id: &'a str,
    pub(crate) publisher_mount_id: u64,
    pub(crate) control_directory_inode: u64,
    pub(crate) filesystem_id: u64,
    pub(crate) filesystem_instance_digest: &'a str,
    pub(crate) device: (u32, u32),
    pub(crate) statx_device: (u32, u32),
    pub(crate) fs_type_magic: u64,
}

pub(crate) fn publication_matches_channel_directory(
    channel: &ProofChannelRecord,
    published: &PublishedChannelDirectory<'_>,
) -> bool {
    let control = channel.control();
    published.volume_name == channel.metadata_volume_name()
        && published.volume_id == channel.metadata_volume_id()
        && published.publisher_mount_id > 0
        && control.mount_id > 0
        && published.control_directory_inode == control.directory_inode
        && published.filesystem_id == control.filesystem_id
        && published.filesystem_instance_digest == control.filesystem_instance_digest
        && published.device == control.device()
        && published.statx_device == control.statx_device()
        && published.fs_type_magic == control.fs_type_magic
}

impl ProofChannelMountRecord {
    fn valid_control(&self) -> bool {
        self.target == CONTROL_TARGET
            && self.subpath == CONTROL_SUBPATH
            && self.valid_common()
            && self.owner_uid == 0
            && self.owner_gid == 0
            && self.mode == 0o555
            && self.read_only
    }

    fn valid_inbox(&self) -> bool {
        const TMPFS_SUPER_MAGIC: u64 = 0x0102_1994;
        self.target == INBOX_TARGET
            && self.subpath == INBOX_SUBPATH
            && self.valid_common()
            && self.owner_uid == 1000
            && self.owner_gid == 1000
            && self.mode == 0o700
            && self.fs_type_magic == TMPFS_SUPER_MAGIC
            && !self.read_only
    }

    fn valid_job_facts(&self) -> bool {
        self.target == JOB_FACTS_TARGET
            && self.subpath == JOB_FACTS_SUBPATH
            && self.valid_common()
            && self.owner_uid == 0
            && self.owner_gid == 0
            && self.mode == 0o555
            && self.read_only
    }

    fn valid_common(&self) -> bool {
        self.mount_id > 0
            && self.fs_type_magic > 0
            && self.filesystem_id > 0
            && valid_digest(&self.filesystem_instance_digest)
            && self.directory_inode > 0
            && self.no_copy
            && ((self.device_major, self.device_minor)
                == (self.statx_device_major, self.statx_device_minor)
                || self.fs_type_magic == BTRFS_SUPER_MAGIC)
    }

    #[cfg(test)]
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }

    #[cfg(test)]
    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
    }

    #[cfg(test)]
    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }

    #[cfg(test)]
    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_object_id(value: &str) -> bool {
    (12..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn valid_volume_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
#[path = "channel_record/tests.rs"]
mod tests;
