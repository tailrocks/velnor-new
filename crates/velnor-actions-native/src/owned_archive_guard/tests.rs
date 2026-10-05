use std::error::Error;
use std::io::{self, Write};

use flate2::Compression as GzipLevel;
use flate2::write::GzEncoder;

use super::{Compression, Profile, validate, validate_with_profile};

const REVISION: &str = "583dddce84706786fc54c41a2c768c28a09c65fd";

fn header(name: &str, kind: u8, size: u64) -> [u8; 512] {
    let mut block = [0_u8; 512];
    let name_bytes = name.as_bytes();
    block[..name_bytes.len()].copy_from_slice(name_bytes);
    write_octal(&mut block[100..108], 0o644);
    write_octal(&mut block[108..116], 0);
    write_octal(&mut block[116..124], 0);
    write_octal(&mut block[124..136], size);
    write_octal(&mut block[136..148], 0);
    block[148..156].fill(b' ');
    block[156] = kind;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    let checksum = block.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let formatted = format!("{checksum:06o}\0 ");
    block[148..156].copy_from_slice(formatted.as_bytes());
    block
}

fn write_octal(field: &mut [u8], value: u64) {
    let text = format!("{value:o}");
    let start = field.len() - text.len() - 1;
    field.fill(b'0');
    field[start..start + text.len()].copy_from_slice(text.as_bytes());
    field[field.len() - 1] = 0;
}

fn append_member(
    archive: &mut Vec<u8>,
    name: &str,
    kind: u8,
    body: &[u8],
) -> Result<(), io::Error> {
    let size = u64::try_from(body.len()).map_err(io::Error::other)?;
    archive.extend_from_slice(&header(name, kind, size));
    archive.extend_from_slice(body);
    let padding = (512 - body.len() % 512) % 512;
    archive.resize(archive.len() + padding, 0);
    Ok(())
}

fn append_member_with_padding(
    archive: &mut Vec<u8>,
    name: &str,
    kind: u8,
    body: &[u8],
    padding_prefix: &[u8],
) -> Result<(), io::Error> {
    let size = u64::try_from(body.len()).map_err(io::Error::other)?;
    let padding = (512 - body.len() % 512) % 512;
    if padding_prefix.len() > padding {
        return Err(io::Error::other("padding prefix exceeds tar padding"));
    }
    archive.extend_from_slice(&header(name, kind, size));
    archive.extend_from_slice(body);
    archive.extend_from_slice(padding_prefix);
    archive.resize(archive.len() + padding - padding_prefix.len(), 0);
    Ok(())
}

fn finish_archive(archive: &mut Vec<u8>) {
    archive.resize(archive.len() + 1024, 0);
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let content = format!("{key}={value}\n");
    let mut length = content.len() + 2;
    loop {
        let next = length.to_string().len() + 1 + content.len();
        if next == length {
            return format!("{length} {content}").into_bytes();
        }
        length = next;
    }
}

fn gzip(archive: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), GzipLevel::default());
    encoder.write_all(archive)?;
    encoder.finish()
}

fn assert_rejected<R: std::io::Read>(
    archive: R,
    mode: &str,
    revision: Option<&str>,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    let problem = match validate(archive, mode, revision) {
        Ok(()) => return Err(io::Error::other("archive unexpectedly passed").into()),
        Err(problem) => problem,
    };
    assert!(problem.contains(needle), "unexpected error: {problem}");
    Ok(())
}

#[test]
fn oversized_compressed_pax_header_is_rejected_before_body_read() -> Result<(), Box<dyn Error>> {
    let compressed = gzip(&header("PaxHeaders/file", b'x', 2 * 1024 * 1024))?;
    assert_rejected(
        compressed.as_slice(),
        "bootstrap",
        None,
        "metadata limit exceeded before allocation",
    )
}

#[test]
fn oversized_gnu_long_name_is_rejected_before_body_read() -> Result<(), Box<dyn Error>> {
    let compressed = gzip(&header("././@LongLink", b'L', 2 * 1024 * 1024))?;
    assert_rejected(
        compressed.as_slice(),
        "bootstrap",
        None,
        "metadata limit exceeded before allocation",
    )
}

#[test]
fn pax_size_override_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(
        &mut archive,
        "PaxHeaders/file",
        b'x',
        &pax_record("size", "9"),
    )?;
    append_member(&mut archive, "file", b'0', b"x")?;
    finish_archive(&mut archive);
    assert_rejected(archive.as_slice(), "source", None, "unsafe PAX key")
}

#[test]
fn pax_sparse_override_and_invalid_record_length_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut sparse = Vec::new();
    append_member(
        &mut sparse,
        "PaxHeaders/file",
        b'x',
        &pax_record("GNU.sparse.map", "0,1"),
    )?;
    finish_archive(&mut sparse);
    assert_rejected(sparse.as_slice(), "source", None, "unsafe PAX key")?;

    let mut malformed = Vec::new();
    append_member(&mut malformed, "PaxHeaders/file", b'x', b"99 path=x\n")?;
    finish_archive(&mut malformed);
    assert_rejected(
        malformed.as_slice(),
        "source",
        None,
        "malformed PAX record boundary",
    )
}

#[test]
fn bounded_path_metadata_and_regular_member_pass() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(
        &mut archive,
        "PaxHeaders/file",
        b'x',
        &pax_record("path", "source/long-name"),
    )?;
    append_member(&mut archive, "file", b'0', b"x")?;
    finish_archive(&mut archive);
    validate(archive.as_slice(), "source", None)?;
    Ok(())
}

#[test]
fn non_file_member_sizes_match_tarfile_consumption() -> Result<(), Box<dyn Error>> {
    for (name, kind) in [("directory", b'5'), ("link", b'2'), ("legacy/", 0)] {
        let mut archive = Vec::new();
        append_member(&mut archive, name, kind, b"hidden")?;
        finish_archive(&mut archive);
        assert_rejected(
            archive.as_slice(),
            "source",
            None,
            "directory and symlink tar members must have zero size",
        )?;
    }

    let mut extended_directory = Vec::new();
    append_member(
        &mut extended_directory,
        "PaxHeaders/file",
        b'x',
        &pax_record("path", "legacy/"),
    )?;
    append_member(&mut extended_directory, "legacy/", 0, b"payload")?;
    finish_archive(&mut extended_directory);
    assert_rejected(
        extended_directory.as_slice(),
        "source",
        None,
        "directory and symlink tar members must have zero size",
    )?;

    let mut extended_regular = Vec::new();
    append_member(
        &mut extended_regular,
        "PaxHeaders/file",
        b'x',
        &pax_record("path", "renamed"),
    )?;
    append_member(&mut extended_regular, "legacy", 0, b"payload")?;
    finish_archive(&mut extended_regular);
    validate(extended_regular.as_slice(), "source", None)?;
    Ok(())
}

#[test]
fn nonzero_pax_padding_cannot_hide_forbidden_metadata() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    let allowed = pax_record("path", "source/file");
    let hidden = pax_record("size", "9");
    append_member_with_padding(&mut archive, "PaxHeaders/file", b'x', &allowed, &hidden)?;
    append_member(&mut archive, "file", b'0', b"x")?;
    finish_archive(&mut archive);
    assert_rejected(
        archive.as_slice(),
        "source",
        None,
        "nonzero tar metadata padding",
    )
}

#[test]
fn nonzero_gnu_longname_padding_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member_with_padding(&mut archive, "././@LongLink", b'L', b"long-name", b"x")?;
    append_member(&mut archive, "long-name", b'0', b"x")?;
    finish_archive(&mut archive);
    assert_rejected(
        gzip(&archive)?.as_slice(),
        "bootstrap",
        None,
        "nonzero tar metadata padding",
    )
}

#[test]
fn candidate_mode_rejects_extension_headers() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(
        &mut archive,
        "PaxHeaders/file",
        b'x',
        &pax_record("path", "file"),
    )?;
    finish_archive(&mut archive);
    assert_rejected(
        gzip(&archive)?.as_slice(),
        "candidate",
        None,
        "PAX tar extensions are forbidden",
    )
}

#[test]
fn sparse_members_and_member_limits_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut sparse = Vec::new();
    append_member(&mut sparse, "file", b'S', b"")?;
    finish_archive(&mut sparse);
    assert_rejected(sparse.as_slice(), "source", None, "GNU sparse")?;

    let mut archive = Vec::new();
    append_member(&mut archive, "one", b'0', b"x")?;
    append_member(&mut archive, "two", b'0', b"y")?;
    finish_archive(&mut archive);
    let policy = Profile {
        compression: Compression::Raw,
        input_limit: 4096,
        archive_limit: 4096,
        payload_limit: 10,
        member_limit: 1,
        allowed_types: 0b011,
        allow_pax: false,
        allow_gnu_long: false,
    };
    let problem = match validate_with_profile(archive.as_slice(), policy, None) {
        Ok(()) => return Err(io::Error::other("member limit unexpectedly passed").into()),
        Err(problem) => problem,
    };
    assert!(problem.contains("member count limit"), "{problem}");
    Ok(())
}

#[test]
fn source_capsule_requires_the_exact_global_comment() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(
        &mut archive,
        "GlobalHead.0",
        b'g',
        &pax_record("comment", REVISION),
    )?;
    append_member(&mut archive, "source/file", b'0', b"x")?;
    finish_archive(&mut archive);
    validate(archive.as_slice(), "semver-capsule", Some(REVISION))?;

    let wrong = pax_record("comment", "0".repeat(40).as_str());
    let mut archive = Vec::new();
    append_member(&mut archive, "GlobalHead.0", b'g', &wrong)?;
    append_member(&mut archive, "source/file", b'0', b"x")?;
    finish_archive(&mut archive);
    assert_rejected(
        archive.as_slice(),
        "semver-capsule",
        Some(REVISION),
        "global PAX metadata differs",
    )
}

#[test]
fn decompression_limit_covers_tail_after_tar_end_markers() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    finish_archive(&mut archive);
    archive.resize(8192, 0);
    let mut policy = Profile {
        compression: Compression::Gzip,
        input_limit: 4096,
        archive_limit: 4096,
        payload_limit: 1024,
        member_limit: 1,
        allowed_types: 0b011,
        allow_pax: false,
        allow_gnu_long: false,
    };
    policy.archive_limit = 2048;
    let compressed = gzip(&archive)?;
    let problem = match validate_with_profile(compressed.as_slice(), policy, None) {
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
        "candidate",
        None,
        "archive read failed",
    )?;

    let mut concatenated = compressed.clone();
    concatenated.extend_from_slice(&gzip(&archive)?);
    assert_rejected(
        concatenated.as_slice(),
        "candidate",
        None,
        "nonzero bytes follow tar end markers",
    )
}

#[test]
fn truncated_tar_and_nonzero_trailing_data_are_rejected() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        [0_u8; 511].as_slice(),
        "source",
        None,
        "truncated tar header",
    )?;
    let mut archive = Vec::new();
    finish_archive(&mut archive);
    archive.push(1);
    assert_rejected(
        archive.as_slice(),
        "source-publication",
        None,
        "nonzero bytes follow tar end markers",
    )
}

#[cfg(test)]
#[path = "additional_tests.rs"]
mod edge_cases;
