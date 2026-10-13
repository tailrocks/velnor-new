use super::super::{ArchiveEncoding, Profile, validate_with_profile};
use super::{append_member, assert_rejected_tar, finish_archive, gzip, pax_record};
use std::error::Error;

fn profile(input_limit: u64, archive_limit: u64, payload_limit: u64) -> Profile {
    Profile {
        encoding: ArchiveEncoding::Gzip,
        input_limit,
        archive_limit,
        payload_limit,
        member_limit: 1024,
        allowed_types: 0b011,
        allow_pax: true,
    }
}

#[test]
fn input_and_payload_limits_are_enforced_at_their_boundaries() -> Result<(), Box<dyn Error>> {
    let exact_input = [0_u8; 1024];
    let compressed = gzip(&exact_input)?;
    validate_with_profile(
        compressed.as_slice(),
        profile(u64::try_from(compressed.len())?, 4096, 0),
    )?;
    let problem = validate_with_profile(
        compressed.as_slice(),
        profile(u64::try_from(compressed.len())? - 1, 4096, 0),
    )
    .expect_err("compressed input one byte over the profile limit must fail");
    assert!(
        problem.contains("compressed input limit exceeded"),
        "{problem}"
    );

    let mut archive = Vec::new();
    append_member(&mut archive, "file", b'0', b"abc")?;
    finish_archive(&mut archive);
    let problem = validate_with_profile(gzip(&archive)?.as_slice(), profile(4096, 4096, 2))
        .expect_err("payload one byte over the profile limit must fail");
    assert!(problem.contains("tar payload limit exceeded"), "{problem}");
    Ok(())
}

#[test]
fn aggregate_metadata_limit_is_enforced_before_the_next_metadata_body() -> Result<(), Box<dyn Error>>
{
    let value = "x".repeat(65_524);
    let record = pax_record("path", &value);
    assert_eq!(record.len(), 65_536);
    let mut archive = Vec::new();
    for index in 0..=256 {
        append_member(&mut archive, "PaxHeaders/file", b'x', &record)?;
        if index < 256 {
            append_member(&mut archive, "file", b'0', b"")?;
        }
    }
    assert_rejected_tar(
        &archive,
        "cargo-package",
        "tar metadata total limit exceeded",
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
    super::assert_rejected(
        compressed.as_slice(),
        "cargo-package",
        "archive read failed",
    )?;
    Ok(())
}
