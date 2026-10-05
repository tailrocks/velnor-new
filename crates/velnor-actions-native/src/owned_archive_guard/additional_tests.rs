use std::error::Error;

use super::super::{Compression, Profile, validate_with_profile};
use super::{append_member, assert_rejected, finish_archive, gzip, pax_record};

fn raw_profile(input_limit: u64, payload_limit: u64) -> Profile {
    Profile {
        compression: Compression::Raw,
        input_limit,
        archive_limit: 4096,
        payload_limit,
        member_limit: 1024,
        allowed_types: 0b011,
        allow_pax: true,
        allow_gnu_long: false,
    }
}

#[test]
fn input_and_payload_limits_are_enforced_at_their_boundaries() -> Result<(), Box<dyn Error>> {
    let exact_input = [0_u8; 1024];
    validate_with_profile(exact_input.as_slice(), raw_profile(1024, 0), None)?;
    let excessive_input = [0_u8; 1025];
    let problem = validate_with_profile(excessive_input.as_slice(), raw_profile(1024, 0), None)
        .expect_err("input one byte over the profile limit must fail");
    assert!(
        problem.contains("archive input limit exceeded"),
        "{problem}"
    );

    let mut archive = Vec::new();
    append_member(&mut archive, "file", b'0', b"abc")?;
    finish_archive(&mut archive);
    let problem = validate_with_profile(archive.as_slice(), raw_profile(4096, 2), None)
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
    assert_rejected(
        archive.as_slice(),
        "source",
        None,
        "tar metadata total limit exceeded",
    )
}

#[test]
fn bad_tar_checksum_is_rejected_before_member_processing() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(&mut archive, "file", b'0', b"payload")?;
    finish_archive(&mut archive);
    archive[0] ^= 1;
    assert_rejected(
        archive.as_slice(),
        "source",
        None,
        "tar header checksum mismatch",
    )
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
        "candidate",
        None,
        "archive read failed",
    )
}

#[test]
fn semver_global_metadata_must_be_followed_by_a_member() -> Result<(), Box<dyn Error>> {
    let revision = "583dddce84706786fc54c41a2c768c28a09c65fd";
    let mut archive = Vec::new();
    append_member(
        &mut archive,
        "GlobalHead.0",
        b'g',
        &pax_record("comment", revision),
    )?;
    finish_archive(&mut archive);
    let problem = validate_with_profile(
        archive.as_slice(),
        super::super::profile::profile("semver-capsule", Some(revision))?,
        Some(revision),
    )
    .expect_err("extension-only archive must fail");
    assert!(
        problem.contains("tar ends after an extension header"),
        "{problem}"
    );
    Ok(())
}
