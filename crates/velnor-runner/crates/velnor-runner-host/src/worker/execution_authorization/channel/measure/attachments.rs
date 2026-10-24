//! Match measured Runner mounts with the storage receipt.

use crate::{
    error::HostError,
    journal::{StorageObserver, StorageRole},
    launch::bootstrap::{BootstrapStartReceipt, VerifiedStorageResourceBinding},
};

use super::super::ProofChannelMount;
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelDirectory, ExecutionChannelObservation,
};

#[cfg(test)]
mod tests;

const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
const CONTROL_SOURCE: &str = "source-proof/control";
const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
const INBOX_SOURCE: &str = "source-proof/inbox";
const JOB_FACTS_PATH: &str = "/opt/velnor/job-facts";
const JOB_FACTS_SOURCE: &str = "job-facts";
const RUNTIME_IDENTITY_PATH: &str = "/.velnor-runtime-identity";
const RUNTIME_IDENTITY_SOURCE: &str = "runtime-identity";

pub(super) struct AttachmentFact {
    role: StorageRole,
    source_target: Option<String>,
    volume_subpath: Option<String>,
    mount_root: String,
    read_only: bool,
    mount_id: u64,
    device: (u32, u32),
    statx_device: (u32, u32),
    fs_type_magic: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
}

impl AttachmentFact {
    pub(super) fn matches(
        &self,
        directory: &ExecutionChannelDirectory,
        target: &str,
        source: &str,
        role: StorageRole,
        read_only: bool,
    ) -> bool {
        let (owner, mode) = if read_only { (0, 0o555) } else { (1000, 0o700) };
        directory.target == target
            && directory.inode > 0
            && directory.owner_uid == owner
            && directory.owner_gid == owner
            && directory.mode == mode
            && directory.mount_id == directory.statx_mount_id
            && directory.read_only == read_only
            && self.role == role
            && self.source_target.is_none()
            && self.volume_subpath.as_deref() == Some(source)
            && self.mount_root == directory.mount_root
            && self.read_only == read_only
            && self.mount_id == directory.mount_id
            && self.device == (directory.mount_device_major, directory.mount_device_minor)
            && self.statx_device == (directory.statx_device_major, directory.statx_device_minor)
            && self.fs_type_magic == directory.filesystem_type_magic
            && self.filesystem_id == directory.filesystem_id
    }
}

pub(super) fn validate_storage_attachments(
    observation: &ExecutionChannelObservation,
    storage: &VerifiedStorageResourceBinding,
    started: &BootstrapStartReceipt,
) -> Result<(), HostError> {
    let control = matching_attachment(
        storage,
        started.runner_container_id(),
        CONTROL_PATH,
        StorageRole::EngineMetadata,
    )?;
    let inbox = matching_attachment(
        storage,
        started.runner_container_id(),
        INBOX_PATH,
        StorageRole::RunnerHome,
    )?;
    let job_facts = matching_attachment(
        storage,
        started.runner_container_id(),
        JOB_FACTS_PATH,
        StorageRole::EngineMetadata,
    )?;
    let job_facts_inode = storage.metadata_job_facts_inode()?;
    let dind_attached = storage.attachments().iter().any(|attachment| {
        attachment.observer() == StorageObserver::Dind
            && attachment.container_id() == Some(started.dind_container_id())
            && dind_channel_mount_is_forbidden(
                attachment.role(),
                attachment.target(),
                attachment.source_target(),
                attachment.volume_subpath(),
                attachment.read_only(),
            )
    });
    if dind_attached
        || !control.matches(
            &observation.control,
            CONTROL_PATH,
            CONTROL_SOURCE,
            StorageRole::EngineMetadata,
            true,
        )
        || !inbox.matches(
            &observation.inbox,
            INBOX_PATH,
            INBOX_SOURCE,
            StorageRole::RunnerHome,
            false,
        )
        || !job_facts.matches(
            &observation.job_facts,
            JOB_FACTS_PATH,
            JOB_FACTS_SOURCE,
            StorageRole::EngineMetadata,
            true,
        )
        || !matches_initialized_job_facts_inode(&observation.job_facts, job_facts_inode)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn matches_initialized_job_facts_inode(
    directory: &ExecutionChannelDirectory,
    initialized_inode: u64,
) -> bool {
    initialized_inode > 0 && directory.inode == initialized_inode
}

fn dind_channel_mount_is_forbidden(
    role: StorageRole,
    target: &str,
    source_target: Option<&str>,
    volume_subpath: Option<&str>,
    read_only: bool,
) -> bool {
    match target {
        CONTROL_PATH | INBOX_PATH => true,
        JOB_FACTS_PATH => {
            role != StorageRole::EngineMetadata
                || source_target.is_some()
                || volume_subpath != Some(JOB_FACTS_SOURCE)
                || !read_only
        }
        RUNTIME_IDENTITY_PATH => {
            role != StorageRole::EngineMetadata
                || source_target.is_some()
                || volume_subpath != Some(RUNTIME_IDENTITY_SOURCE)
                || !read_only
        }
        _ => false,
    }
}

pub(super) fn matching_attachment(
    storage: &VerifiedStorageResourceBinding,
    container_id: &str,
    target: &str,
    role: StorageRole,
) -> Result<AttachmentFact, HostError> {
    let mut matches = storage.attachments().iter().filter(|attachment| {
        attachment.role() == role
            && attachment.observer() == StorageObserver::Runner
            && attachment.container_id() == Some(container_id)
            && attachment.target() == target
    });
    let attachment = matches.next().ok_or(HostError::Identity)?;
    if matches.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(AttachmentFact {
        role: attachment.role(),
        source_target: attachment.source_target().map(str::to_owned),
        volume_subpath: attachment.volume_subpath().map(str::to_owned),
        mount_root: attachment.mount_root().to_owned(),
        read_only: attachment.read_only(),
        mount_id: attachment.mount_id(),
        device: attachment.device(),
        statx_device: attachment.statx_device(),
        fs_type_magic: attachment.fs_type_magic(),
        filesystem_id: attachment.filesystem_id(),
        filesystem_instance_digest: attachment.filesystem_instance_digest().to_owned(),
    })
}

pub(super) fn make_mount(
    directory: &ExecutionChannelDirectory,
    target: &str,
    source: &str,
    storage: &VerifiedStorageResourceBinding,
    runner_container_id: &str,
    read_only: bool,
    role: StorageRole,
) -> Result<ProofChannelMount, HostError> {
    let attachment = matching_attachment(storage, runner_container_id, target, role)?;
    if !attachment.matches(directory, target, source, role, read_only) {
        return Err(HostError::Identity);
    }
    Ok(ProofChannelMount {
        target: target.to_owned(),
        subpath: source.to_owned(),
        mount_id: directory.mount_id,
        device_major: directory.mount_device_major,
        device_minor: directory.mount_device_minor,
        statx_device_major: directory.statx_device_major,
        statx_device_minor: directory.statx_device_minor,
        fs_type_magic: directory.filesystem_type_magic,
        filesystem_id: directory.filesystem_id,
        filesystem_instance_digest: attachment.filesystem_instance_digest,
        directory_inode: directory.inode,
        owner_uid: directory.owner_uid,
        owner_gid: directory.owner_gid,
        mode: directory.mode,
        read_only,
        no_copy: true,
    })
}
