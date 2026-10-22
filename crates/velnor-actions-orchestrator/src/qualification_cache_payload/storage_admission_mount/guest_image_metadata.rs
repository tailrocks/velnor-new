use super::{MAX_GUEST_IMAGE_BYTES, ROOT_UID};

#[derive(Clone, Copy)]
pub(super) struct GuestImageFileMetadata {
    pub(super) is_regular_file: bool,
    pub(super) owner_uid: u32,
    pub(super) mode: u32,
    pub(super) has_single_link: bool,
    pub(super) size_bytes: i64,
}

impl GuestImageFileMetadata {
    pub(super) fn from_stat(stat: &rustix::fs::Stat) -> Self {
        use rustix::fs::FileType;

        Self {
            is_regular_file: FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile,
            owner_uid: stat.st_uid,
            mode: stat.st_mode & 0o7777,
            has_single_link: stat.st_nlink == 1,
            size_bytes: stat.st_size,
        }
    }

    pub(super) fn validate(self) -> Result<(), &'static str> {
        if !self.is_regular_file
            || self.owner_uid != ROOT_UID
            || self.mode != 0o444
            || !self.has_single_link
            || self.size_bytes <= 0
            || u64::try_from(self.size_bytes).unwrap_or(u64::MAX) > MAX_GUEST_IMAGE_BYTES as u64
        {
            Err("cache_storage_guest_image_file_invalid")
        } else {
            Ok(())
        }
    }
}
