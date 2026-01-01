use std::error::Error;
use std::io;

use super::super::profile::{ArchiveEncoding, Profile};
use super::super::validate_with_profile;
use super::{append_member, assert_rejected, assert_rejected_tar, finish_archive, gzip};

#[test]
fn decompression_limit_covers_tail_after_tar_end_markers() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    finish_archive(&mut archive);
    archive.resize(8192, 0);
    let mut policy = Profile {
        encoding: ArchiveEncoding::Gzip,
        input_limit: 4096,
        archive_limit: 4096,
        payload_limit: 1024,
        member_limit: 1,
        allowed_types: 0b011,
        allow_pax: false,
    };
    policy.archive_limit = 2048;
    let compressed = gzip(&archive)?;
    let problem = match validate_with_profile(compressed.as_slice(), policy) {
        Ok(()) => return Err(io::Error::other("expanded archive unexpectedly passed").into()),
        Err(problem) => problem,
    };
    assert!(
        problem.contains("gzip expansion limit exceeded"),
        "{problem}"
    );
    Ok(())
}

#[test]
fn truncated_and_concatenated_gzip_streams_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(&mut archive, "file", b'0', b"payload")?;
    finish_archive(&mut archive);
    let compressed = gzip(&archive)?;
    assert_rejected(
        &compressed[..compressed.len() - 5],
        "cargo-package",
        "archive read failed",
    )?;

    let mut concatenated = compressed.clone();
    concatenated.extend_from_slice(&gzip(&archive)?);
    assert_rejected(
        concatenated.as_slice(),
        "cargo-package",
        "nonzero bytes follow tar end markers",
    )
}

#[test]
fn truncated_tar_and_nonzero_trailing_data_are_rejected() -> Result<(), Box<dyn Error>> {
    assert_rejected_tar(&[0_u8; 511], "cargo-package", "truncated tar header")?;
    let mut archive = Vec::new();
    finish_archive(&mut archive);
    archive.push(1);
    assert_rejected_tar(
        &archive,
        "cargo-package",
        "nonzero bytes follow tar end markers",
    )
}
#[test]
fn bad_tar_checksum_is_rejected_before_member_processing() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(&mut archive, "file", b'0', b"payload")?;
    finish_archive(&mut archive);
    archive[0] ^= 1;
    assert_rejected_tar(&archive, "cargo-package", "tar header checksum mismatch")
}

#[test]
fn gzip_crc_corruption_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(&mut archive, "file", b'0', b"payload")?;
    finish_archive(&mut archive);
    let mut compressed = gzip(&archive)?;
    let crc_offset = compressed.len() - 8;
    compressed[crc_offset] ^= 1;
    assert_rejected(
        compressed.as_slice(),
        "cargo-package",
        "archive read failed",
    )?;
    Ok(())
}
