use super::super::profile::PaxMode;
use super::super::{ArchiveEncoding, Profile, validate, validate_with_profile};
use super::{
    append_member, assert_rejected_raw_tar, assert_rejected_tar, finish_archive, gzip, pax_record,
};
use std::error::Error;

fn profile(input_limit: u64, archive_limit: u64, payload_limit: u64) -> Profile {
    Profile {
        encoding: ArchiveEncoding::Gzip,
        input_limit,
        archive_limit,
        payload_limit,
        member_limit: 1024,
        allowed_types: 0b011,
        pax_mode: PaxMode::Safe,
    }
}

#[test]
fn cache_payload_profile_has_fixed_raw_tar_bounds_and_member_types() -> Result<(), Box<dyn Error>> {
    let policy = super::super::profile("cache-payload")?;
    assert!(matches!(policy.encoding, ArchiveEncoding::RawTar));
    assert_eq!(policy.input_limit, 20 * 1024 * 1024 * 1024);
    assert_eq!(policy.archive_limit, 20 * 1024 * 1024 * 1024);
    assert_eq!(policy.payload_limit, 16 * 1024 * 1024 * 1024);
    assert_eq!(policy.member_limit, 250_000);
    assert_eq!(policy.allowed_types, 0b1111);
    assert!(matches!(policy.pax_mode, PaxMode::CachePaths));
    Ok(())
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

#[test]
fn cache_payload_accepts_writer_path_pax_and_hardlink_records() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    let mut file_path = pax_record("path", "roots/tofu_plugins/providers/long-provider-name");
    append_member(&mut archive, "PaxHeaders/file", b'x', &file_path)?;
    append_member(&mut archive, "entry", b'0', b"provider")?;

    file_path.clear();
    file_path.extend_from_slice(&pax_record("path", "roots/tofu_plugins/providers/copy"));
    file_path.extend_from_slice(&pax_record(
        "linkpath",
        "roots/tofu_plugins/providers/long-provider-name",
    ));
    append_member(&mut archive, "PaxHeaders/link", b'x', &file_path)?;
    append_member(&mut archive, "link", b'1', b"")?;
    finish_archive(&mut archive);

    validate(archive.as_slice(), "cache-payload").map_err(std::io::Error::other)?;
    Ok(())
}

#[test]
fn cache_payload_rejects_other_pax_keys_duplicate_keys_and_nonzero_hardlinks()
-> Result<(), Box<dyn Error>> {
    let mut unsupported_key = Vec::new();
    append_member(
        &mut unsupported_key,
        "PaxHeaders/file",
        b'x',
        &pax_record("mtime", "0"),
    )?;
    append_member(&mut unsupported_key, "file", b'0', b"x")?;
    finish_archive(&mut unsupported_key);
    assert_rejected_raw_tar(
        &unsupported_key,
        "cache-payload",
        "unsupported PAX key for cache payload",
    )?;

    let mut duplicate_key = Vec::new();
    let mut duplicate_path = pax_record("path", "one");
    duplicate_path.extend_from_slice(&pax_record("path", "two"));
    append_member(&mut duplicate_key, "PaxHeaders/file", b'x', &duplicate_path)?;
    append_member(&mut duplicate_key, "file", b'0', b"x")?;
    finish_archive(&mut duplicate_key);
    assert_rejected_raw_tar(
        &duplicate_key,
        "cache-payload",
        "duplicate PAX key for cache payload",
    )?;

    let mut sized_hardlink = Vec::new();
    append_member(&mut sized_hardlink, "link", b'1', b"x")?;
    finish_archive(&mut sized_hardlink);
    assert_rejected_raw_tar(
        &sized_hardlink,
        "cache-payload",
        "hardlink tar members must have zero size",
    )
}

#[test]
fn cache_payload_rejects_malformed_and_global_pax_headers_and_bad_checksums()
-> Result<(), Box<dyn Error>> {
    let mut malformed = Vec::new();
    append_member(&mut malformed, "PaxHeaders/file", b'x', b"99 path=x\n")?;
    finish_archive(&mut malformed);
    assert_rejected_raw_tar(&malformed, "cache-payload", "malformed PAX record boundary")?;

    let mut global = Vec::new();
    append_member(
        &mut global,
        "GlobalHead",
        b'g',
        &pax_record("path", "roots/tofu_plugins/providers/file"),
    )?;
    finish_archive(&mut global);
    assert_rejected_raw_tar(
        &global,
        "cache-payload",
        "global PAX tar extensions are forbidden",
    )?;

    let mut bad_checksum = Vec::new();
    append_member(&mut bad_checksum, "file", b'0', b"payload")?;
    finish_archive(&mut bad_checksum);
    bad_checksum[0] ^= 1;
    assert_rejected_raw_tar(
        &bad_checksum,
        "cache-payload",
        "tar header checksum mismatch",
    )
}

#[test]
fn cache_payload_rejects_gzip_and_other_profiles_still_reject_hardlinks()
-> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(&mut archive, "link", b'1', b"")?;
    finish_archive(&mut archive);
    let compressed = gzip(&archive)?;

    let raw_error = validate(compressed.as_slice(), "cache-payload")
        .expect_err("cache payload profile accepts only raw tar");
    assert_ne!(raw_error, "");
    assert_rejected_raw_tar(&archive, "candidate", "unsupported tar member type")?;
    assert_rejected_tar(&archive, "cargo-package", "unsupported tar member type")?;
    Ok(())
}
