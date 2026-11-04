// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
};
use velnor_actions_orchestrator::fd_tool_extract::extract_verified_tool_archive_file;
use zip::{ZipWriter, write::SimpleFileOptions};

#[test]
fn verified_archive_installs_exact_tree_and_never_overwrites()
-> Result<(), Box<dyn std::error::Error>> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(7);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(
        &mut header,
        format!("toolchains/1.97.1-{target}/bin/cargo"),
        &b"fixture"[..],
    )?;
    let tar = builder.into_inner()?.finish()?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&tar)?;
    let bytes = zip.finish()?.into_inner();
    let hash = hex_digest(&bytes);
    let archive = root.join("archive.zip");
    fs::write(&archive, bytes)?;
    let file = fs::File::open(&archive)?;
    let dest = root.join("rustup");
    assert!(extract_verified_tool_archive_file(&file, &"0".repeat(64), "1.97.1", &dest).is_err());
    assert!(!dest.exists());
    extract_verified_tool_archive_file(&file, &hash, "1.97.1", &dest)?;
    assert_eq!(
        fs::read(dest.join(format!("toolchains/1.97.1-{target}/bin/cargo")))?,
        b"fixture"
    );
    assert!(extract_verified_tool_archive_file(&file, &hash, "1.97.1", &dest).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn archive_rejects_duplicates_links_unsafe_modes_and_trailing_streams()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let root = tempfile::tempdir()?;
    let root_path = root.path().canonicalize()?;
    let name = format!("toolchains/1.97.1-{target}/bin/cargo");
    for (case, entries) in [
        (
            "duplicate",
            vec![
                (name.clone(), tar::EntryType::Regular, 0o755),
                (name.clone(), tar::EntryType::Regular, 0o755),
            ],
        ),
        ("link", vec![(name.clone(), tar::EntryType::Symlink, 0o755)]),
        ("special", vec![(name.clone(), tar::EntryType::Fifo, 0o755)]),
        (
            "worldwrite",
            vec![(name.clone(), tar::EntryType::Regular, 0o777)],
        ),
        (
            "suid",
            vec![(name.clone(), tar::EntryType::Regular, 0o4755)],
        ),
        (
            "case-parent",
            vec![
                (
                    name.replace("bin/cargo", "Foo/one"),
                    tar::EntryType::Regular,
                    0o755,
                ),
                (
                    name.replace("bin/cargo", "foo/two"),
                    tar::EntryType::Regular,
                    0o755,
                ),
            ],
        ),
        (
            "case",
            vec![
                (name.clone(), tar::EntryType::Regular, 0o755),
                (name.to_ascii_uppercase(), tar::EntryType::Regular, 0o755),
            ],
        ),
    ] {
        let gzip = make_tar(&entries)?;
        assert_rejected(&root_path, case, &gzip)?;
    }
    let valid = make_tar(&[(name, tar::EntryType::Regular, 0o755)])?;
    let mut appended = valid.clone();
    appended.extend_from_slice(b"junk");
    assert_rejected(&root_path, "trailing", &appended)?;
    let mut concatenated = valid.clone();
    concatenated.extend_from_slice(&valid);
    assert_rejected(&root_path, "concat", &concatenated)?;
    let mut plain = Vec::new();
    flate2::read::GzDecoder::new(valid.as_slice()).read_to_end(&mut plain)?;
    plain.extend_from_slice(b"bad tar tail");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&plain)?;
    assert_rejected(&root_path, "tar-tail", &encoder.finish()?)?;
    Ok(())
}
fn make_tar(
    entries: &[(String, tar::EntryType, u32)],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (name, kind, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(*kind);
        header.set_size(if kind.is_file() { 7 } else { 0 });
        header.set_mode(*mode);
        if kind.is_symlink() {
            header.set_link_name("../../outside")?;
        }
        header.set_cksum();
        let content: &[u8] = if kind.is_file() { b"fixture" } else { b"" };
        tar.append_data(&mut header, name, content)?;
    }
    Ok(tar.into_inner()?.finish()?)
}
fn assert_rejected(
    root: &std::path::Path,
    case: &str,
    gzip: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(gzip)?;
    let bytes = zip.finish()?.into_inner();
    let hash = hex_digest(&bytes);
    let archive = root.join(format!("{case}.zip"));
    fs::write(&archive, bytes)?;
    let dest = root.join(format!("dest-{case}"));
    assert!(
        extract_verified_tool_archive_file(&fs::File::open(archive)?, &hash, "1.97.1", &dest)
            .is_err(),
        "{case} must fail"
    );
    assert!(!dest.exists(), "{case} must not install a tree");
    Ok(())
}

#[cfg(unix)]
#[test]
fn accepts_gnu_long_names_and_rejects_escaping_parent() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let root = tempfile::tempdir()?;
    let root = root.path().canonicalize()?;
    let name = format!(
        "toolchains/1.97.1-{target}/{}/cargo",
        "long-name".repeat(20)
    );
    let gzip = make_tar(&[(name.clone(), tar::EntryType::Regular, 0o755)])?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&gzip)?;
    let bytes = zip.finish()?.into_inner();
    let hash = hex_digest(&bytes);
    let archive = root.join("long.zip");
    fs::write(&archive, bytes)?;
    let dest = root.join("long-dest");
    extract_verified_tool_archive_file(&fs::File::open(&archive)?, &hash, "1.97.1", &dest)?;
    assert_eq!(fs::read(dest.join(name))?, b"fixture");
    let outside = tempfile::tempdir()?;
    let alias = root.join("alias");
    symlink(outside.path(), &alias)?;
    assert!(
        extract_verified_tool_archive_file(
            &fs::File::open(&archive)?,
            &hash,
            "1.97.1",
            &alias.join("rustup")
        )
        .is_err()
    );
    assert!(!outside.path().join("rustup").exists());
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut result = String::new();
    for byte in Sha256::digest(bytes) {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    result
}

#[cfg(unix)]
#[test]
fn pax_path_is_applied_but_conflicting_metadata_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let name = format!("toolchains/1.97.1-{target}/bin/pax-cargo");
    let path_record = pax_record("path", &name);
    let valid = pax_tar(&path_record)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&valid)?;
    let bytes = zip.finish()?.into_inner();
    let hash = hex_digest(&bytes);
    let archive = root.join("pax.zip");
    fs::write(&archive, bytes)?;
    let dest = root.join("pax-dest");
    extract_verified_tool_archive_file(&fs::File::open(archive)?, &hash, "1.97.1", &dest)?;
    assert_eq!(fs::read(dest.join(&name))?, b"fixture");
    let traversal = format!("{name}/../../outside");
    assert_rejected(
        &root,
        "pax-traversal",
        &pax_tar(&pax_record("path", &traversal))?,
    )?;
    let duplicate = format!("{path_record}{path_record}");
    assert_rejected(&root, "pax-duplicate", &pax_tar(&duplicate)?)?;
    assert_rejected(&root, "pax-size", &pax_tar(&pax_record("size", "1"))?)?;
    assert_rejected(
        &root,
        "pax-sparse",
        &pax_tar(&pax_record("GNU.sparse.size", "1"))?,
    )?;
    Ok(())
}
fn pax_record(key: &str, value: &str) -> String {
    let body = format!("{key}={value}\n");
    let mut len = body.len() + 2;
    loop {
        let next = len.to_string().len() + 1 + body.len();
        if next == len {
            break;
        }
        len = next;
    }
    format!("{len} {body}")
}
fn pax_tar(record: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::XHeader);
    header.set_size(record.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append_data(&mut header, "pax", record.as_bytes())?;
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(7);
    header.set_mode(0o755);
    header.set_cksum();
    archive.append_data(&mut header, "raw", &b"fixture"[..])?;
    Ok(archive.into_inner()?.finish()?)
}

#[cfg(unix)]
#[test]
fn many_directories_extract_without_holding_them_all_open() -> Result<(), Box<dyn std::error::Error>>
{
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let root = tempfile::tempdir()?;
    let root = root.path().canonicalize()?;
    let entries: Vec<_> = (0..300)
        .map(|index| {
            (
                format!("toolchains/1.97.1-{target}/dir-{index:03}/file"),
                tar::EntryType::Regular,
                0o644,
            )
        })
        .collect();
    let gzip = make_tar(&entries)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&gzip)?;
    let bytes = zip.finish()?.into_inner();
    let hash = hex_digest(&bytes);
    let archive = root.join("dirs.zip");
    fs::write(&archive, bytes)?;
    let dest = root.join("rustup");
    extract_verified_tool_archive_file(&fs::File::open(archive)?, &hash, "1.97.1", &dest)?;
    assert_eq!(fs::read(dest.join(&entries[299].0))?, b"fixture");
    Ok(())
}

#[cfg(unix)]
#[test]
fn archive_with_wrong_zip_crc_is_not_installed() -> Result<(), Box<dyn std::error::Error>> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let name = format!("toolchains/1.97.1-{target}/bin/cargo");
    let gzip = make_tar(&[(name, tar::EntryType::Regular, 0o755)])?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&gzip)?;
    let mut bytes = zip.finish()?.into_inner();
    let central = bytes
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .ok_or("missing central directory")?;
    bytes[central + 16] ^= 1;
    let hash = hex_digest(&bytes);
    let archive = root.join("bad-crc.zip");
    fs::write(&archive, bytes)?;
    let dest = root.join("rustup");
    assert!(
        extract_verified_tool_archive_file(&fs::File::open(archive)?, &hash, "1.97.1", &dest)
            .is_err()
    );
    assert!(!dest.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn outer_zip_special_file_is_not_installed() -> Result<(), Box<dyn std::error::Error>> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let name = format!("toolchains/1.97.1-{target}/bin/cargo");
    let gzip = make_tar(&[(name, tar::EntryType::Regular, 0o755)])?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("rust-toolchain.tar.gz", SimpleFileOptions::default())?;
    zip.write_all(&gzip)?;
    let mut bytes = zip.finish()?.into_inner();
    let central = bytes
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .ok_or("missing central directory")?;
    bytes[central + 5] = 3;
    bytes[central + 38..central + 42].copy_from_slice(&(0o010_644u32 << 16).to_le_bytes());
    let hash = hex_digest(&bytes);
    let archive = root.join("outer-fifo.zip");
    fs::write(&archive, bytes)?;
    let dest = root.join("rustup");
    assert!(
        extract_verified_tool_archive_file(&fs::File::open(archive)?, &hash, "1.97.1", &dest)
            .is_err()
    );
    assert!(!dest.exists());
    Ok(())
}
