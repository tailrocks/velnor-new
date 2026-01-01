use std::error::Error;
use std::io;

use super::super::profile::{ArchiveEncoding, Profile};
use super::super::validate_with_profile;
use super::{
    append_member, append_member_with_padding, assert_rejected, assert_rejected_raw_tar,
    assert_rejected_tar, finish_archive, gzip, header, pax_record, validate_tar,
};

#[test]
fn oversized_compressed_pax_header_is_rejected_before_body_read() -> Result<(), Box<dyn Error>> {
    let compressed = gzip(&header("PaxHeaders/file", b'x', 2 * 1024 * 1024))?;
    assert_rejected(
        compressed.as_slice(),
        "cargo-package",
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
    assert_rejected_tar(archive.as_slice(), "cargo-package", "unsafe PAX key")
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
    assert_rejected_tar(sparse.as_slice(), "cargo-package", "unsafe PAX key")?;

    let mut malformed = Vec::new();
    append_member(&mut malformed, "PaxHeaders/file", b'x', b"99 path=x\n")?;
    finish_archive(&mut malformed);
    assert_rejected_tar(&malformed, "cargo-package", "malformed PAX record boundary")
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
    validate_tar(archive.as_slice(), "cargo-package")?;
    Ok(())
}

#[test]
fn non_file_member_sizes_match_tarfile_consumption() -> Result<(), Box<dyn Error>> {
    for (name, kind, needle) in [
        (
            "directory",
            b'5',
            "directory and symlink tar members must have zero size",
        ),
        ("link", b'2', "unsupported tar member type"),
        (
            "legacy/",
            0,
            "directory and symlink tar members must have zero size",
        ),
    ] {
        let mut archive = Vec::new();
        append_member(&mut archive, name, kind, b"hidden")?;
        finish_archive(&mut archive);
        assert_rejected_tar(&archive, "cargo-package", needle)?;
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
    assert_rejected_tar(
        &extended_directory,
        "cargo-package",
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
    validate_tar(extended_regular.as_slice(), "cargo-package")?;
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
    assert_rejected_tar(&archive, "cargo-package", "nonzero tar metadata padding")
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
    assert_rejected_raw_tar(&archive, "candidate", "PAX tar extensions are forbidden")
}

#[test]
fn cargo_package_rejects_gnu_long_headers() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(&mut archive, "././@LongLink", b'L', b"long-name")?;
    finish_archive(&mut archive);
    assert_rejected_tar(
        &archive,
        "cargo-package",
        "GNU long tar extension is forbidden",
    )
}

#[test]
fn sparse_members_and_member_limits_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut sparse = Vec::new();
    append_member(&mut sparse, "file", b'S', b"")?;
    finish_archive(&mut sparse);
    assert_rejected_tar(&sparse, "cargo-package", "GNU sparse")?;

    let mut archive = Vec::new();
    append_member(&mut archive, "one", b'0', b"x")?;
    append_member(&mut archive, "two", b'0', b"y")?;
    finish_archive(&mut archive);
    let policy = Profile {
        encoding: ArchiveEncoding::Gzip,
        input_limit: 4096,
        archive_limit: 4096,
        payload_limit: 10,
        member_limit: 1,
        allowed_types: 0b011,
        allow_pax: false,
    };
    let problem = match validate_with_profile(gzip(&archive)?.as_slice(), policy) {
        Ok(()) => return Err(io::Error::other("member limit unexpectedly passed").into()),
        Err(problem) => problem,
    };
    assert!(problem.contains("member count limit"), "{problem}");
    Ok(())
}

#[test]
fn global_pax_headers_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut archive = Vec::new();
    append_member(
        &mut archive,
        "GlobalHead.0",
        b'g',
        &pax_record("comment", "unused"),
    )?;
    append_member(&mut archive, "source/file", b'0', b"x")?;
    finish_archive(&mut archive);
    assert_rejected_tar(
        &archive,
        "cargo-package",
        "global PAX tar extensions are forbidden",
    )
}
