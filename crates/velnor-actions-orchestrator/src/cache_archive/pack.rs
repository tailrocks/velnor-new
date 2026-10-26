// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};
use velnor_actions_contract::canonical_json_bytes;

use super::types::{MAX_ARCHIVE_ENTRIES, PackRequest, PackedArchive};
use crate::OrchestratorError;
use crate::cache_snapshot::{
    LaneRoot, MANIFEST_ARCHIVE_MEMBER, PROJECTION_ARCHIVE_PREFIX,
    PROJECTION_INDEX_ARCHIVE_MEMBER, REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER, STATE_ARCHIVE_PREFIX,
    STATE_INDEX_ARCHIVE_MEMBER, SnapshotPlan, StateEntryV1, archived_projection_records,
    manifest_canonical_bytes, validate_state_index,
};
use crate::internal::{internal, internal_contract};
use crate::source_prep::cache_source_projection::{
    EntryKind, ProjectionEntry, sha256_hex, validate_link_target_in_root,
};

pub(super) fn io_error(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn member_header(mode: u32, kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode & 0o777);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
}

struct PackWriter<W: Write> {
    builder: Builder<W>,
    count: usize,
}

impl<W: Write> PackWriter<W> {
    fn admit(&mut self) -> Result<(), OrchestratorError> {
        self.count = self.count.saturating_add(1);
        if self.count > MAX_ARCHIVE_ENTRIES {
            return Err(internal("snapshot_archive_entry_limit"));
        }
        Ok(())
    }

    fn append_json(&mut self, name: &str, bytes: &[u8]) -> Result<(), OrchestratorError> {
        self.admit()?;
        let size = u64::try_from(bytes.len()).map_err(|_| internal("snapshot_size_range"))?;
        let mut header = member_header(0o644, EntryType::Regular, size);
        self.builder
            .append_data(&mut header, name, bytes)
            .map_err(|error| internal(&format!("snapshot_archive_write:{error}")))?;
        Ok(())
    }

    fn append_file(
        &mut self,
        name: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), OrchestratorError> {
        self.admit()?;
        let size = u64::try_from(bytes.len()).map_err(|_| internal("snapshot_size_range"))?;
        let mut header = member_header(mode, EntryType::Regular, size);
        self.builder
            .append_data(&mut header, name, bytes)
            .map_err(|error| internal(&format!("snapshot_archive_write:{error}")))?;
        Ok(())
    }

    fn append_directory(&mut self, name: &str, mode: u32) -> Result<(), OrchestratorError> {
        self.admit()?;
        let mut header = member_header(mode, EntryType::Directory, 0);
        self.builder
            .append_data(&mut header, name, std::io::empty())
            .map_err(|error| internal(&format!("snapshot_archive_write:{error}")))?;
        Ok(())
    }

    fn append_link(
        &mut self,
        name: &str,
        target: &Path,
        mode: u32,
    ) -> Result<(), OrchestratorError> {
        self.admit()?;
        let mut header = member_header(mode, EntryType::Symlink, 0);
        self.builder
            .append_link(&mut header, name, target)
            .map_err(|error| internal(&format!("snapshot_archive_write:{error}")))?;
        Ok(())
    }
}

fn lane_fs_root<'a>(
    lanes: &'a [LaneRoot],
    lane_id: &str,
    member: &str,
) -> Result<&'a Path, OrchestratorError> {
    lanes
        .iter()
        .find(|lane| lane.lane_id == lane_id)
        .map(|lane| lane.root.as_path())
        .ok_or_else(|| internal(&format!("snapshot_archive_lane:{member}")))
}

fn split_state_member(member: &str) -> Result<(&str, &str), OrchestratorError> {
    let rest = member
        .strip_prefix(STATE_ARCHIVE_PREFIX)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or_else(|| internal("snapshot_archive_prefix"))?;
    rest.split_once('/')
        .ok_or_else(|| internal("snapshot_archive_lane"))
}

fn pack_state_entry<W: Write>(
    writer: &mut PackWriter<W>,
    lanes: &[LaneRoot],
    entry: &StateEntryV1,
) -> Result<(), OrchestratorError> {
    let (lane_id, lane_relative) = split_state_member(&entry.path)?;
    let lane_root = lane_fs_root(lanes, lane_id, &entry.path)?;
    let absolute = lane_root.join(lane_relative);
    match entry.entry_kind()? {
        EntryKind::File => {
            let bytes = fs::read(&absolute).map_err(|error| io_error(&absolute, error))?;
            if sha256_hex(&bytes) != entry.content_sha256 {
                return Err(internal("snapshot_archive_content_mismatch"));
            }
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != entry.size {
                return Err(internal("snapshot_archive_size_mismatch"));
            }
            writer.append_file(&entry.path, &bytes, entry.mode)?;
        }
        EntryKind::Directory => {
            if !absolute.is_dir() {
                return Err(internal("snapshot_archive_kind_mismatch"));
            }
            writer.append_directory(&entry.path, entry.mode)?;
        }
        EntryKind::Symlink => {
            let Some(target) = entry.link_target.as_deref() else {
                return Err(internal("snapshot_link_missing"));
            };
            let actual = fs::read_link(&absolute).map_err(|error| io_error(&absolute, error))?;
            let text = actual
                .to_str()
                .ok_or_else(|| internal("snapshot_link_not_utf8"))?;
            if text != target {
                return Err(internal("snapshot_archive_link_mismatch"));
            }
            validate_link_target_in_root(lane_root, lane_relative, text)
                .map_err(|_| internal("snapshot_archive_link_escape"))?;
            writer.append_link(&entry.path, Path::new(target), entry.mode)?;
        }
    }
    Ok(())
}

fn pack_projection_entry<W: Write>(
    writer: &mut PackWriter<W>,
    checkout_root: &Path,
    staged_root: &Path,
    lane_relative: &ProjectionEntry,
    staged: bool,
) -> Result<(), OrchestratorError> {
    let root = if staged { staged_root } else { checkout_root };
    let absolute = root.join(&lane_relative.path);
    let member = format!("{PROJECTION_ARCHIVE_PREFIX}/{}", lane_relative.path);
    match lane_relative.kind {
        EntryKind::File => {
            let bytes = fs::read(&absolute).map_err(|error| io_error(&absolute, error))?;
            if sha256_hex(&bytes) != lane_relative.content_sha256 {
                return Err(internal("snapshot_archive_content_mismatch"));
            }
            writer.append_file(&member, &bytes, lane_relative.mode)?;
        }
        EntryKind::Directory => {
            if !absolute.is_dir() {
                return Err(internal("snapshot_archive_kind_mismatch"));
            }
            writer.append_directory(&member, lane_relative.mode)?;
        }
        EntryKind::Symlink => {
            let Some(target) = lane_relative.link_target.as_deref() else {
                return Err(internal("snapshot_link_missing"));
            };
            let actual = fs::read_link(&absolute).map_err(|error| io_error(&absolute, error))?;
            let text = actual
                .to_str()
                .ok_or_else(|| internal("snapshot_link_not_utf8"))?;
            if text != target || sha256_hex(text.as_bytes()) != lane_relative.content_sha256 {
                return Err(internal("snapshot_archive_link_mismatch"));
            }
            validate_link_target_in_root(root, &lane_relative.path, text)
                .map_err(|_| internal("snapshot_archive_link_escape"))?;
            writer.append_link(&member, Path::new(target), lane_relative.mode)?;
        }
    }
    Ok(())
}

fn validate_pack_request(request: &PackRequest<'_>) -> Result<(), OrchestratorError> {
    let plan = request.plan;
    plan.manifest.validate()?;
    validate_state_index(&plan.state_index)?;
    if !request.checkout_root.is_absolute() || !request.staged_projection_root.is_absolute() {
        return Err(internal("snapshot_archive_root_not_absolute"));
    }
    if !request.checkout_root.is_dir() || !request.staged_projection_root.is_dir() {
        return Err(internal("snapshot_archive_root_not_directory"));
    }
    for lane in &plan.lanes {
        lane.validate()?;
        if !lane.root.is_dir() {
            return Err(internal("snapshot_lane_not_directory"));
        }
    }
    let parent = request.out_path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(internal("snapshot_archive_parent_missing"));
    }
    if fs::symlink_metadata(request.out_path).is_ok() {
        return Err(OrchestratorError::OverwriteRefused {
            path: request.out_path.display().to_string(),
        });
    }
    Ok(())
}

fn append_snapshot_indexes<W: Write>(
    writer: &mut PackWriter<W>,
    plan: &SnapshotPlan,
) -> Result<(), OrchestratorError> {
    writer.append_json(
        MANIFEST_ARCHIVE_MEMBER,
        &manifest_canonical_bytes(&plan.manifest)?,
    )?;
    let state_bytes = canonical_json_bytes(&plan.state_index).map_err(internal_contract)?;
    writer.append_json(STATE_INDEX_ARCHIVE_MEMBER, &state_bytes)?;
    let projection_records = archived_projection_records(
        &plan
            .projection
            .iter()
            .map(|entry| entry.record.clone())
            .collect::<Vec<_>>(),
    )?;
    let projection_bytes = canonical_json_bytes(&projection_records).map_err(internal_contract)?;
    writer.append_json(PROJECTION_INDEX_ARCHIVE_MEMBER, &projection_bytes)?;
    let required_bytes = canonical_json_bytes(&plan.required_outputs).map_err(internal_contract)?;
    writer.append_json(REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER, &required_bytes)?;
    Ok(())
}

pub fn pack_snapshot_archive(
    request: &PackRequest<'_>,
) -> Result<PackedArchive, OrchestratorError> {
    let plan = request.plan;
    validate_pack_request(request)?;
    let file =
        File::create_new(request.out_path).map_err(|error| io_error(request.out_path, error))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut writer = PackWriter {
        builder: Builder::new(encoder),
        count: 0,
    };
    append_snapshot_indexes(&mut writer, plan)?;
    for entry in &plan.state_index {
        pack_state_entry(&mut writer, &plan.lanes, entry)?;
    }
    let mut ordered_projection = plan.projection.clone();
    ordered_projection.sort_by(|left, right| left.record.path.cmp(&right.record.path));
    for entry in &ordered_projection {
        pack_projection_entry(
            &mut writer,
            request.checkout_root,
            request.staged_projection_root,
            &entry.record,
            entry.staged,
        )?;
    }
    let member_count = writer.count;
    let encoder = writer
        .builder
        .into_inner()
        .map_err(|error| internal(&format!("snapshot_archive_finish:{error}")))?;
    let mut file = encoder
        .finish()
        .map_err(|error| internal(&format!("snapshot_archive_finish:{error}")))?;
    file.flush()
        .map_err(|error| io_error(request.out_path, error))?;
    file.sync_all()
        .map_err(|error| io_error(request.out_path, error))?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(request.out_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error(request.out_path, error))?;
    }
    let bytes = fs::read(request.out_path).map_err(|error| io_error(request.out_path, error))?;
    Ok(PackedArchive {
        archive_sha256: sha256_hex(&bytes),
        archive_bytes: u64::try_from(bytes.len()).map_err(|_| internal("snapshot_size_range"))?,
        member_count,
    })
}
