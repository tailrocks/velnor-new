use super::{StorageObserver, StorageRole};

/// Mount record for one observed attachment. Shared tmpfs volumes have one physical role row and
/// separate Runner/DinD attachments with the same backing filesystem digest.
pub(crate) struct ObservedStorageAttachment {
    pub(super) role: StorageRole,
    pub(super) observer: StorageObserver,
    pub(super) container_id: Option<String>,
    pub(super) target: String,
    pub(super) source_target: Option<String>,
    pub(super) volume_subpath: Option<String>,
    pub(super) mount_root: String,
    pub(super) read_only: bool,
    pub(super) mount_id: u64,
    pub(super) statx_inode: u64,
    pub(super) device_major: u32,
    pub(super) device_minor: u32,
    pub(super) statx_device_major: u32,
    pub(super) statx_device_minor: u32,
    pub(super) fs_type_magic: u64,
    pub(super) filesystem_id: u64,
    pub(super) filesystem_instance_digest: String,
}
impl ObservedStorageAttachment {
    pub(crate) const fn role(&self) -> StorageRole {
        self.role
    }

    pub(crate) const fn observer(&self) -> StorageObserver {
        self.observer
    }

    pub(crate) fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn source_target(&self) -> Option<&str> {
        self.source_target.as_deref()
    }

    pub(crate) fn volume_subpath(&self) -> Option<&str> {
        self.volume_subpath.as_deref()
    }

    pub(crate) fn mount_root(&self) -> &str {
        &self.mount_root
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn statx_inode(&self) -> u64 {
        self.statx_inode
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
}
