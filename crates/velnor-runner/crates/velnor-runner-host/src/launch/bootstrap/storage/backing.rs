#[cfg(test)]
use super::ObservedStorageAttachment;
use super::StorageRole;

/// Link a container's logical root mount to its measured backing filesystem.
pub(crate) struct VerifiedRootBackingMapping {
    pub(super) reservation_id: i64,
    pub(super) operation_id: i64,
    pub(super) connection_id: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) role: StorageRole,
    pub(super) container_id: String,
    pub(super) image_runtime_id: String,
    pub(super) image_source_sha: String,
    pub(super) logical_mount_id: u64,
    pub(super) logical_device: (u32, u32),
    pub(super) logical_statx_device: (u32, u32),
    pub(super) logical_fs_type_magic: u64,
    pub(super) logical_filesystem_id: u64,
    pub(super) logical_filesystem_instance_digest: String,
    pub(super) backing_filesystem_instance_digest: String,
    pub(super) canonical_record: String,
    pub(super) mapping_digest: String,
}
impl VerifiedRootBackingMapping {
    #[cfg(test)]
    pub(crate) fn test_fixture(
        reservation_id: i64,
        operation_id: i64,
        connection_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        sample_generation_id: &str,
        role: StorageRole,
        container_id: &str,
        image_runtime_id: &str,
        image_source_sha: &str,
        attachment: &ObservedStorageAttachment,
        backing_filesystem_instance_digest: &str,
    ) -> Self {
        use sha2::{Digest, Sha256};

        let canonical_record = serde_json::to_string(&serde_json::json!({
            "schema_version": 1,
            "reservation_id": reservation_id,
            "operation_id": operation_id,
            "connection_id": connection_id,
            "engine_id": engine_id,
            "docker_root_digest": docker_root_digest,
            "sample_generation_id": sample_generation_id,
            "role": role.as_str(),
            "container_id": container_id,
            "image_runtime_id": image_runtime_id,
            "image_source_sha": image_source_sha,
            "logical": {
                "mount_id": attachment.mount_id(),
                "mount_source": "overlay",
                "device": attachment.device(),
                "statx_device": attachment.statx_device(),
                "fs_type_magic": attachment.fs_type_magic(),
                "filesystem_id": attachment.filesystem_id(),
                "filesystem_digest": attachment.filesystem_instance_digest(),
            },
            "backing": {
                "mount_source": "/dev/vdb",
                "filesystem_instance_digest": backing_filesystem_instance_digest,
            },
        }))
        .expect("test root backing record serializes");
        let mut digest = Sha256::new();
        digest.update(b"velnor-root-backing-map-v1\0");
        digest.update(canonical_record.as_bytes());
        let mapping_digest = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self {
            reservation_id,
            operation_id,
            connection_id: connection_id.to_owned(),
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
            sample_generation_id: sample_generation_id.to_owned(),
            role,
            container_id: container_id.to_owned(),
            image_runtime_id: image_runtime_id.to_owned(),
            image_source_sha: image_source_sha.to_owned(),
            logical_mount_id: attachment.mount_id(),
            logical_device: attachment.device(),
            logical_statx_device: attachment.statx_device(),
            logical_fs_type_magic: attachment.fs_type_magic(),
            logical_filesystem_id: attachment.filesystem_id(),
            logical_filesystem_instance_digest: attachment.filesystem_instance_digest().to_owned(),
            backing_filesystem_instance_digest: backing_filesystem_instance_digest.to_owned(),
            canonical_record,
            mapping_digest,
        }
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
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }
    pub(crate) const fn role(&self) -> StorageRole {
        self.role
    }
    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }
    pub(crate) fn image_runtime_id(&self) -> &str {
        &self.image_runtime_id
    }
    pub(crate) fn image_source_sha(&self) -> &str {
        &self.image_source_sha
    }
    pub(crate) const fn logical_mount_id(&self) -> u64 {
        self.logical_mount_id
    }
    pub(crate) const fn logical_device(&self) -> (u32, u32) {
        self.logical_device
    }
    pub(crate) const fn logical_statx_device(&self) -> (u32, u32) {
        self.logical_statx_device
    }
    pub(crate) const fn logical_fs_type_magic(&self) -> u64 {
        self.logical_fs_type_magic
    }
    pub(crate) const fn logical_filesystem_id(&self) -> u64 {
        self.logical_filesystem_id
    }
    pub(crate) fn logical_filesystem_digest(&self) -> &str {
        &self.logical_filesystem_instance_digest
    }
    pub(crate) fn backing_filesystem_digest(&self) -> &str {
        &self.backing_filesystem_instance_digest
    }
    pub(crate) fn canonical_record(&self) -> &str {
        &self.canonical_record
    }
    pub(crate) fn mapping_digest(&self) -> &str {
        &self.mapping_digest
    }
}
