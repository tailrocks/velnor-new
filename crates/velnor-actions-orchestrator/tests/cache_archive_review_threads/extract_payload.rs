// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use tar::EntryType;
use velnor_actions_orchestrator::extract_snapshot_archive;

use super::{expect_code, fail, gzip_bytes, member_header};

#[test]
fn review_directory_payload_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let archive = temp.path().join("dir-payload.tar.gz");
    let bytes = gzip_bytes(|builder| {
        let mut header = member_header(0o755, EntryType::Directory, 5);
        builder
            .append_data(&mut header, "state/build/subdir", b"XXXXX".as_slice())
            .map_err(|error| fail(&error.to_string()))?;
        Ok(())
    })?;
    fs::write(&archive, bytes)?;
    let destination = temp.path().join("restore");
    let error = extract_snapshot_archive(&archive, &destination)
        .expect_err("directory payload must be rejected");
    expect_code(&error, "snapshot_archive_nonfile_payload")?;
    Ok(())
}

#[test]
fn review_symlink_payload_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let archive = temp.path().join("link-payload.tar.gz");
    let bytes = gzip_bytes(|builder| {
        let mut header = member_header(0o777, EntryType::Symlink, 4);
        header
            .set_link_name("app")
            .map_err(|error| fail(&error.to_string()))?;
        builder
            .append_data(&mut header, "state/build/latest", b"YYYY".as_slice())
            .map_err(|error| fail(&error.to_string()))?;
        Ok(())
    })?;
    fs::write(&archive, bytes)?;
    let destination = temp.path().join("restore");
    let error = extract_snapshot_archive(&archive, &destination)
        .expect_err("symlink payload must be rejected");
    expect_code(&error, "snapshot_archive_nonfile_payload")?;
    Ok(())
}
