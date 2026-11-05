// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn tool_archive_requires_verified_single_tree_and_refuses_overwrite()
-> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{Cursor, Write},
        os::unix::fs::PermissionsExt,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};
    let temp = tempfile::tempdir()?;
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let tar_bytes = test_toolchain_gzip(target)?;
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&tar_bytes)?;
    let bytes = zip.finish()?.into_inner();
    let mut digest = String::new();
    for byte in Sha256::digest(&bytes) {
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    let archive = temp.path().join("artifact.zip");
    fs::write(&archive, bytes)?;
    let destination = temp.path().canonicalize()?.join("rustup");
    assert!(extract_fixture(&archive, &"0".repeat(64), "1.97.1", &destination).is_err());
    assert!(!destination.exists());
    let mut bad_tar = tar_bytes.clone();
    bad_tar.extend_from_slice(b"unexpected trailing stream");
    assert_rejected_gzip(&temp, &destination, &bad_tar, "trailing.zip")?;
    let mut concatenated = tar_bytes.clone();
    concatenated.extend_from_slice(&tar_bytes);
    assert_rejected_gzip(&temp, &destination, &concatenated, "concatenated.zip")?;
    let mut decompressed = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(tar_bytes.as_slice()),
        &mut decompressed,
    )?;
    decompressed.extend_from_slice(b"nonzero tar tail");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&decompressed)?;
    assert_rejected_gzip(&temp, &destination, &encoder.finish()?, "tar-tail.zip")?;
    extract_fixture(&archive, &digest, "1.97.1", &destination)?;
    assert_eq!(
        fs::read(destination.join(format!("toolchains/1.97.1-{target}/bin/cargo")))?,
        b"fixture"
    );
    assert_ne!(
        fs::metadata(destination.join(format!("toolchains/1.97.1-{target}/bin/cargo")))?
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert!(extract_fixture(&archive, &digest, "1.97.1", &destination).is_err());
    let handle = fs::File::open(&archive)?;
    fs::remove_file(&archive)?;
    let other_destination = temp.path().canonicalize()?.join("rustup-from-handle");
    velnor_actions_orchestrator::crate_release_tool_artifact::extract_verified_tool_archive_file(
        &handle,
        &digest,
        "1.97.1",
        &other_destination,
        tool_deadline(),
    )?;
    assert_eq!(
        fs::read(other_destination.join(format!("toolchains/1.97.1-{target}/bin/cargo")))?,
        b"fixture"
    );
    Ok(())
}

#[cfg(unix)]
fn test_toolchain_gzip(target: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    let gz = GzEncoder::new(Vec::new(), Compression::fast());
    let mut tar = tar::Builder::new(gz);
    let mut header = tar::Header::new_gnu();
    header.set_size(7);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("toolchains/1.97.1-{target}/bin/cargo"),
        &b"fixture"[..],
    )?;
    let tar_bytes = tar.into_inner()?.finish()?;
    Ok(tar_bytes)
}

#[cfg(unix)]
fn assert_rejected_gzip(
    temp: &tempfile::TempDir,
    destination: &std::path::Path,
    gzip: &[u8],
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "rust-toolchain.tar.gz",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(gzip)?;
    let bytes = zip.finish()?.into_inner();
    let mut digest = String::new();
    for byte in Sha256::digest(&bytes) {
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    let path = temp.path().join(name);
    std::fs::write(&path, bytes)?;
    assert!(extract_fixture(&path, &digest, "1.97.1", destination).is_err());
    assert!(!destination.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn toolchain_archive_rejects_links_and_wrong_tree_without_promotion()
-> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    let temp = tempfile::tempdir()?;
    let destination = temp.path().canonicalize()?.join("rustup");
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut regular = tar::Header::new_gnu();
    regular.set_size(1);
    regular.set_mode(0o755);
    regular.set_cksum();
    tar.append_data(
        &mut regular,
        format!("toolchains/1.97.1-{target}/bin/cargo"),
        &b"x"[..],
    )?;
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_link_name("cargo")?;
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("toolchains/1.97.1-{target}/bin/other"),
        &[][..],
    )?;
    let gzip = tar.into_inner()?.finish()?;
    assert_rejected_gzip(&temp, &destination, &gzip, "link.zip")?;
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("toolchains/1.97.2-{target}/bin/cargo"),
        &b"x"[..],
    )?;
    let gzip = tar.into_inner()?.finish()?;
    assert_rejected_gzip(&temp, &destination, &gzip, "wrong-tree.zip")?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn hostile_outer_zip_is_never_promoted() -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{Cursor, Write},
    };
    let temp = tempfile::tempdir()?;
    let destination = temp.path().canonicalize()?.join("rustup");
    for (index, name) in [
        "../escape",
        "/absolute",
        "rust-toolchain.tar.gz/../escape",
        "wrong-name",
    ]
    .iter()
    .enumerate()
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(*name, zip::write::SimpleFileOptions::default())?;
        zip.write_all(b"junk")?;
        let bytes = zip.finish()?.into_inner();
        let mut digest = String::new();
        for byte in Sha256::digest(&bytes) {
            digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        }
        let path = temp.path().join(format!("bad-{index}.zip"));
        fs::write(&path, bytes)?;
        assert!(extract_fixture(&path, &digest, "1.97.1", &destination).is_err());
        assert!(!destination.exists());
        assert!(!temp.path().join("escape").exists());
    }
    let huge = temp.path().join("huge.zip");
    fs::File::create(&huge)?.set_len(1024 * 1024 * 1024 + 1)?;
    assert!(extract_fixture(&huge, &"0".repeat(64), "1.97.1", &destination).is_err());
    assert!(!destination.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn hostile_inner_tar_is_never_promoted() -> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    let temp = tempfile::tempdir()?;
    let destination = temp.path().canonicalize()?.join("rustup");
    for (index, path) in [
        "../escape",
        "/absolute",
        "toolchains/../../escape",
        "toolchains\\escape",
    ]
    .iter()
    .enumerate()
    {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        let raw = header.as_mut_bytes();
        raw[..100].fill(0);
        raw[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        tar.append(&header, &b"x"[..])?;
        assert_rejected_gzip(
            &temp,
            &destination,
            &tar.into_inner()?.finish()?,
            &format!("hostile-{index}.zip"),
        )?;
        assert!(!temp.path().join("escape").exists());
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    for (index, kind) in [
        tar::EntryType::Link,
        tar::EntryType::Fifo,
        tar::EntryType::Char,
    ]
    .iter()
    .enumerate()
    {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(*kind);
        header.set_size(0);
        header.set_mode(0o644);
        header.set_link_name("cargo")?;
        header.set_cksum();
        tar.append_data(
            &mut header,
            format!("toolchains/1.97.1-{target}/bin/bad"),
            &[][..],
        )?;
        assert_rejected_gzip(
            &temp,
            &destination,
            &tar.into_inner()?.finish()?,
            &format!("special-{index}.zip"),
        )?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn toolchain_outer_zip_must_have_exactly_one_member() -> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{Cursor, Write},
    };
    let temp = tempfile::tempdir()?;
    let destination = temp.path().canonicalize()?.join("rustup");
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("toolchains/1.97.1-{host}/bin/cargo"),
        &b"x"[..],
    )?;
    let gzip = tar.into_inner()?.finish()?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "rust-toolchain.tar.gz",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(&gzip)?;
    zip.start_file("extra", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"unwanted")?;
    let bytes = zip.finish()?.into_inner();
    let mut digest = String::new();
    for byte in Sha256::digest(&bytes) {
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    let archive = temp.path().join("extra.zip");
    fs::write(&archive, bytes)?;
    assert!(extract_fixture(&archive, &digest, "1.97.1", &destination).is_err());
    assert!(!destination.exists());
    Ok(())
}

fn extract_fixture(
    path: &std::path::Path,
    digest: &str,
    version: &str,
    destination: &std::path::Path,
) -> Result<(), velnor_actions_orchestrator::OrchestratorError> {
    let file = std::fs::File::open(path).map_err(|_| {
        velnor_actions_orchestrator::OrchestratorError::Contract {
            problem: "test archive open".into(),
        }
    })?;
    velnor_actions_orchestrator::crate_release_tool_artifact::extract_verified_tool_archive_file(
        &file,
        digest,
        version,
        destination,
        tool_deadline(),
    )
}

fn tool_deadline() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(60)
}
