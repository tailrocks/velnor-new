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

    pub(crate) fn job_facts(&self) -> &ProofChannelMountRecord {
        &self.job_facts
    }

    pub(crate) fn observation(&self) -> &ExecutionChannelObservation {
        &self.observation
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

    pub(crate) const fn dind_attachment_absent(&self) -> bool {
        self.dind_attachment_absent
    }

    pub(crate) fn control(&self) -> &ProofChannelMountRecord {
        &self.control
    }

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

pub(crate) fn publication_matches_channel_directory(
    control: &ProofChannelMountRecord,
    expected_volume_name: &str,
    expected_volume_id: &str,
    volume_name: &str,
    volume_id: &str,
    publisher_mount_id: u64,
    control_directory_inode: u64,
    filesystem_id: u64,
    filesystem_instance_digest: &str,
    device: (u32, u32),
    statx_device: (u32, u32),
    fs_type_magic: u64,
) -> bool {
    volume_name == expected_volume_name
        && volume_id == expected_volume_id
        && publisher_mount_id > 0
        && control.mount_id > 0
        && control_directory_inode == control.directory_inode
        && filesystem_id == control.filesystem_id
        && filesystem_instance_digest == control.filesystem_instance_digest
        && device == control.device()
        && statx_device == control.statx_device()
        && fs_type_magic == control.fs_type_magic
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

    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }

    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
    }

    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) fn directory_inode(&self) -> u64 {
        self.directory_inode
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn subpath(&self) -> &str {
        &self.subpath
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
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
mod tests {
    use super::*;

    #[test]
    fn publication_accepts_namespace_local_mount_ids_for_same_volume_directory() {
        let channel = fixture();
        let control = channel.control();
        assert!(publication_matches_channel_directory(
            control,
            channel.metadata_volume_name(),
            channel.metadata_volume_id(),
            channel.metadata_volume_name(),
            channel.metadata_volume_id(),
            999,
            control.directory_inode,
            control.filesystem_id(),
            control.filesystem_instance_digest(),
            control.device(),
            control.statx_device(),
            control.fs_type_magic(),
        ));
    }

    #[test]
    fn publication_rejects_wrong_volume_even_when_mount_facts_match() {
        let channel = fixture();
        let control = channel.control();
        assert!(!publication_matches_channel_directory(
            control,
            channel.metadata_volume_name(),
            channel.metadata_volume_id(),
            "foreign-volume",
            channel.metadata_volume_id(),
            control.mount_id(),
            control.directory_inode,
            control.filesystem_id(),
            control.filesystem_instance_digest(),
            control.device(),
            control.statx_device(),
            control.fs_type_magic(),
        ));
    }

    fn fixture() -> ProofChannelRecord {
        ProofChannelRecord {
            schema_version: 3,
            reservation_id: 7,
            operation_id: 11,
            connection_id: "connection-a".to_owned(),
            scale_set_id: 13,
            witness_request_id: 100,
            engine_id: "engine-a".to_owned(),
            docker_root_digest: "a".repeat(64),
            runner_id: 17,
            runner_name: "vnonce".to_owned(),
            runner_container_id: "runner-container-0001".to_owned(),
            dind_container_id: "dind-container-00001".to_owned(),
            instance_binding_digest: "b".repeat(64),
            metadata_volume_name: "wmetadata".to_owned(),
            metadata_volume_id: "metadata-volume-0001".to_owned(),
            control: mount_record(
                CONTROL_TARGET,
                CONTROL_SUBPATH,
                21,
                31,
                101,
                0,
                0,
                0o555,
                true,
            ),
            inbox: mount_record(
                INBOX_TARGET,
                INBOX_SUBPATH,
                22,
                32,
                102,
                1000,
                1000,
                0o700,
                false,
            ),
            job_facts: mount_record(
                JOB_FACTS_TARGET,
                JOB_FACTS_SUBPATH,
                23,
                33,
                103,
                0,
                0,
                0o555,
                true,
            ),
            observation: ExecutionChannelObservation {
                schema_version: 3,
                operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
                guest_boot_id: "11111111-2222-4333-8444-555555555555".to_owned(),
                mount_namespace_id: 44,
                control: directory_record(
                    CONTROL_TARGET,
                    101,
                    21,
                    8,
                    1,
                    0xef53,
                    31,
                    true,
                    "ext4",
                    "/dev/vdb",
                ),
                inbox: directory_record(
                    INBOX_TARGET,
                    102,
                    22,
                    0,
                    42,
                    0x0102_1994,
                    32,
                    false,
                    "tmpfs",
                    "tmpfs",
                ),
                job_facts: directory_record(
                    JOB_FACTS_TARGET,
                    103,
                    23,
                    8,
                    1,
                    0xef53,
                    33,
                    true,
                    "ext4",
                    "/dev/vdb",
                ),
            },
            dind_attachment_absent: true,
        }
    }

    fn mount_record(
        target: &str,
        subpath: &str,
        mount_id: u64,
        filesystem_id: u64,
        directory_inode: u64,
        owner_uid: u32,
        owner_gid: u32,
        mode: u32,
        read_only: bool,
    ) -> ProofChannelMountRecord {
        ProofChannelMountRecord {
            target: target.to_owned(),
            subpath: subpath.to_owned(),
            mount_id,
            device_major: if target == INBOX_TARGET { 0 } else { 8 },
            device_minor: if target == INBOX_TARGET { 42 } else { 1 },
            statx_device_major: if target == INBOX_TARGET { 0 } else { 8 },
            statx_device_minor: if target == INBOX_TARGET { 42 } else { 1 },
            fs_type_magic: if target == INBOX_TARGET {
                0x0102_1994
            } else {
                0xef53
            },
            filesystem_id,
            filesystem_instance_digest: "c".repeat(64),
            directory_inode,
            owner_uid,
            owner_gid,
            mode,
            read_only,
            no_copy: true,
        }
    }

    fn directory_record(
        target: &str,
        inode: u64,
        mount_id: u64,
        device_major: u32,
        device_minor: u32,
        fs_type_magic: u64,
        filesystem_id: u64,
        read_only: bool,
        filesystem_type_name: &str,
        mount_source: &str,
    ) -> velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
        velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
            target: target.to_owned(),
            inode,
            owner_uid: if target == INBOX_TARGET { 1000 } else { 0 },
            owner_gid: if target == INBOX_TARGET { 1000 } else { 0 },
            mode: if target == INBOX_TARGET { 0o700 } else { 0o555 },
            mount_id,
            statx_mount_id: mount_id,
            mount_root: match target {
                CONTROL_TARGET => "/volume/source-proof/control",
                INBOX_TARGET => "/source-proof/inbox",
                JOB_FACTS_TARGET => "/volume/job-facts",
                _ => "/invalid",
            }
            .to_owned(),
            mount_source: mount_source.to_owned(),
            mount_device_major: device_major,
            mount_device_minor: device_minor,
            statx_device_major: device_major,
            statx_device_minor: device_minor,
            filesystem_type_magic: fs_type_magic,
            filesystem_type_name: filesystem_type_name.to_owned(),
            filesystem_id,
            read_only,
            executable_mount: false,
        }
    }

    fn serialized(record: &ProofChannelRecord) -> (String, String) {
        let bytes = record.canonical_json().expect("canonical channel JSON");
        let json = String::from_utf8(bytes.clone()).expect("UTF-8 channel JSON");
        (json, digest(&bytes))
    }

    #[test]
    fn canonical_channel_record_reopens_with_exact_mount_facts() {
        let (json, expected_digest) = serialized(&fixture());
        let reopened = ProofChannelRecord::parse(&json, &expected_digest)
            .expect("the canonical proof channel must reopen");

        assert!(reopened.valid());
        assert_eq!(reopened.reservation_id(), 7);
        assert_eq!(reopened.witness_request_id(), 100);
        assert_eq!(reopened.control().mode, 0o555);
        assert_eq!(reopened.inbox().mode, 0o700);
    }

    #[test]
    fn channel_record_rejects_changed_digest_or_noncanonical_json() {
        let (json, expected_digest) = serialized(&fixture());
        assert!(ProofChannelRecord::parse(&json, &"d".repeat(64)).is_err());

        let noncanonical = format!(" {json}");
        assert!(ProofChannelRecord::parse(&noncanonical, &expected_digest).is_err());
    }

    #[test]
    fn channel_record_rejects_wrong_target_owner_mode_and_dind_attachment() {
        let wrong_target = mutate(|record| record.control.target = "/tmp/control".to_owned());
        assert!(!wrong_target.valid());

        let wrong_owner = mutate(|record| record.inbox.owner_uid = 0);
        assert!(!wrong_owner.valid());

        let wrong_mode = mutate(|record| record.control.mode = 0o4555);
        assert!(!wrong_mode.valid());

        let dind_attached = mutate(|record| record.dind_attachment_absent = false);
        assert!(!dind_attached.valid());
    }

    #[test]
    fn channel_record_rejects_changed_job_facts_mount_and_directory() {
        let mut wrong_mount = fixture();
        wrong_mount.job_facts.subpath = "source-proof/control".to_owned();
        assert!(!wrong_mount.valid());

        let mut wrong_directory = fixture();
        wrong_directory.observation.job_facts.mount_root = "/foreign/../job-facts".to_owned();
        assert!(!wrong_directory.valid());

        let mut writable_directory = fixture();
        writable_directory.observation.job_facts.read_only = false;
        assert!(!writable_directory.valid());
    }

    #[test]
    fn channel_record_accepts_distinct_mount_filesystems_and_rejects_changed_observation() {
        assert!(fixture().valid());
        let same_directory = mutate(|record| {
            record.inbox.directory_inode = record.control.directory_inode;
        });
        assert!(!same_directory.valid());

        let changed_observation = mutate(|record| record.observation.inbox.filesystem_id += 1);
        assert!(!changed_observation.valid());
    }

    fn mutate(change: impl FnOnce(&mut ProofChannelRecord)) -> ProofChannelRecord {
        let mut record = fixture();
        change(&mut record);
        record
    }
}
