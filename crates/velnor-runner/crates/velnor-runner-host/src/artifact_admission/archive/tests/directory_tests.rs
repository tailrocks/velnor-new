use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::super::tar_archive::read_archive;
use super::super::validate;
use crate::error::HostError;

#[test]
fn accepts_docker_directory_headers_with_or_without_trailing_slashes() -> Result<(), HostError> {
    for directories in [
        ["blobs/", "blobs/sha256/"],
        ["blobs/", "blobs/sha256"],
        ["blobs", "blobs/sha256/"],
    ] {
        let (archive, mut manifest) = super::fixture(None, false)?;
        let files = read_archive(&archive)?;
        let archive = archive_with_members(&files, &directories, &[])?;
        manifest.archive_sha256 = super::lowercase_hex(&Sha256::digest(&archive));
        validate(&archive, &manifest)?;
    }
    Ok(())
}

#[test]
fn rejects_duplicate_directory_aliases_in_either_order() -> Result<(), HostError> {
    let files = fixture_files()?;
    for directories in [
        ["blobs", "blobs/", "blobs/sha256"],
        ["blobs/", "blobs", "blobs/sha256"],
        ["blobs", "blobs/sha256", "blobs/sha256/"],
        ["blobs/", "blobs/sha256/", "blobs/sha256"],
        ["blobs", "blobs", "blobs/sha256"],
    ] {
        let archive = archive_with_members(&files, &directories, &[])?;
        assert_eq!(read_archive(&archive), Err(HostError::Identity));
    }
    Ok(())
}

#[test]
fn rejects_noncanonical_directory_header_paths() -> Result<(), HostError> {
    let files = fixture_files()?;
    for name in [
        "blobs//",
        "./blobs",
        "/blobs",
        "blobs/../blobs",
        "blobs/./sha256/",
        "blobs/../sha256/",
        "blobs//sha256/",
        "blobs/sha256//",
        "blobs/cache/",
    ] {
        let directories = if name.contains("sha256") {
            ["blobs/", name]
        } else {
            [name, "blobs/sha256/"]
        };
        let archive = archive_with_members(&files, &directories, &[])?;
        assert_eq!(read_archive(&archive), Err(HostError::Identity), "{name}");
    }
    Ok(())
}

#[test]
fn rejects_nonempty_directory_and_directory_type_masquerades() -> Result<(), HostError> {
    let files = fixture_files()?;
    let archive = archive_with_members(
        &files,
        &["blobs/sha256"],
        &[("blobs/", tar::EntryType::Directory, b"x")],
    )?;
    assert_eq!(read_archive(&archive), Err(HostError::Identity));

    let archive = archive_with_members(
        &files,
        &["blobs/sha256/"],
        &[("blobs", tar::EntryType::Regular, b"")],
    )?;
    assert_eq!(read_archive(&archive), Err(HostError::Identity));

    for link_type in [tar::EntryType::Symlink, tar::EntryType::Link] {
        let archive =
            archive_with_members(&files, &["blobs/sha256/"], &[("blobs", link_type, b"")])?;
        assert_eq!(read_archive(&archive), Err(HostError::Identity));

        let mut files = files.clone();
        files.remove("manifest.json");
        let archive = archive_with_members(
            &files,
            &["blobs/", "blobs/sha256/"],
            &[("manifest.json", link_type, b"")],
        )?;
        assert_eq!(read_archive(&archive), Err(HostError::Identity));
    }
    Ok(())
}

fn fixture_files() -> Result<BTreeMap<String, Vec<u8>>, HostError> {
    let (archive, _) = super::fixture(None, false)?;
    read_archive(&archive)
}

fn archive_with_members(
    files: &BTreeMap<String, Vec<u8>>,
    directories: &[&str],
    extra: &[(&str, tar::EntryType, &[u8])],
) -> Result<Vec<u8>, HostError> {
    let mut builder = tar::Builder::new(Vec::new());
    for name in directories {
        append(&mut builder, name, tar::EntryType::Directory, b"")?;
    }
    for (name, bytes) in files {
        append(&mut builder, name, tar::EntryType::Regular, bytes)?;
    }
    for (name, kind, bytes) in extra {
        append(&mut builder, name, *kind, bytes)?;
    }
    builder.finish().map_err(|_| HostError::Identity)?;
    builder.into_inner().map_err(|_| HostError::Identity)
}

fn append(
    builder: &mut tar::Builder<Vec<u8>>,
    name: &str,
    kind: tar::EntryType,
    bytes: &[u8],
) -> Result<(), HostError> {
    let mut header = tar::Header::new_gnu();
    let name_bytes = name.as_bytes();
    if name_bytes.len() > 100 {
        return Err(HostError::Identity);
    }
    header.as_mut_bytes()[..name_bytes.len()].copy_from_slice(name_bytes);
    header.set_entry_type(kind);
    header.set_mode(0o644);
    header.set_size(u64::try_from(bytes.len()).map_err(|_| HostError::Frame)?);
    header.set_cksum();
    builder
        .append(&header, bytes)
        .map_err(|_| HostError::Identity)
}
