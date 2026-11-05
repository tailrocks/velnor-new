// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cursor, SimpleFileOptions, Write, ZipWriter, extract_verified_tool_archive_file, fs,
    hex_digest, make_tar, tool_deadline,
};

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
        extract_verified_tool_archive_file(
            &fs::File::open(archive)?,
            &hash,
            "1.97.1",
            &dest,
            tool_deadline()
        )
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
        extract_verified_tool_archive_file(
            &fs::File::open(archive)?,
            &hash,
            "1.97.1",
            &dest,
            tool_deadline()
        )
        .is_err()
    );
    assert!(!dest.exists());
    Ok(())
}
