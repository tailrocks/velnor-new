use crate::error::HostError;

use super::values::{nonnegative, positive_u64};

pub(super) struct Attachment {
    pub(super) mount_id: u64,
    pub(super) device: (u32, u32),
    pub(super) statx_device: (u32, u32),
    pub(super) fs_type_magic: u64,
    pub(super) filesystem_id: u64,
}

pub(super) async fn read_attachment(
    connection: &turso::Connection,
    reservation_id: i64,
    role: &str,
    observer: &str,
    container_id: Option<&str>,
    target: &str,
) -> Result<Attachment, HostError> {
    let mut rows = connection.query(
        "SELECT mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, filesystem_id FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = ?1 AND role = ?2 AND observer = ?3 AND container_id IS ?4 AND mount_target = ?5",
        (reservation_id, role, observer, container_id, target),
    ).await.map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let filesystem_id: String = row.get(6).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Attachment {
        mount_id: positive_u64(row.get(0).map_err(|_| HostError::Journal)?)?,
        device: (to_u32(&row, 1)?, to_u32(&row, 2)?),
        statx_device: (to_u32(&row, 3)?, to_u32(&row, 4)?),
        fs_type_magic: nonnegative(row.get(5).map_err(|_| HostError::Journal)?)?,
        filesystem_id: u64::from_str_radix(&filesystem_id, 16).map_err(|_| HostError::Journal)?,
    })
}

fn to_u32(row: &turso::Row, index: usize) -> Result<u32, HostError> {
    u32::try_from(nonnegative(
        row.get(index).map_err(|_| HostError::Journal)?,
    )?)
    .map_err(|_| HostError::Journal)
}
