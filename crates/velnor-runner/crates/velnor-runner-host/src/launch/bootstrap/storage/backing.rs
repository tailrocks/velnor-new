// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::StorageRole;

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
