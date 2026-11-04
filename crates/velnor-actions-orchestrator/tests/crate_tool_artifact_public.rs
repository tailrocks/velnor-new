// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_actions_orchestrator::crate_release_tool_artifact::validate_tool_artifact_metadata;

fn valid() -> (Value, Value) {
    let artifact = json!({"id":10,"name":"velnor-release-rust-12-1","digest":format!("sha256:{}", "a".repeat(64)),"size_in_bytes":400_000_000u64,"expired":false,"workflow_run":{"id":12,"repository_id":22,"head_repository_id":22,"head_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}});
    (
        json!({"total_count":1,"artifacts":[artifact.clone()]}),
        artifact,
    )
}
fn check(list: &Value, detail: &Value) -> bool {
    validate_tool_artifact_metadata(
        list,
        detail,
        10,
        12,
        1,
        22,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .is_ok()
}
#[test]
fn tool_artifact_must_have_unique_current_run_identity_and_digest() {
    let (mut list, mut detail) = valid();
    assert!(check(&list, &detail));
    detail["workflow_run"]["id"] = json!(13);
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["workflow_run"]["repository_id"] = json!(23);
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["workflow_run"]["head_repository_id"] = json!(23);
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["workflow_run"]["head_sha"] = json!("c".repeat(40));
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["expired"] = json!(true);
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["name"] = json!("velnor-release-rust-12-2");
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["digest"] = json!("sha256:bad");
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    list["total_count"] = json!(2);
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    list["artifacts"]
        .as_array_mut()
        .expect("array")
        .push(detail.clone());
    list["total_count"] = json!(2);
    assert!(!check(&list, &detail));
    (list, detail) = valid();
    detail["size_in_bytes"] = json!(u64::MAX);
    assert!(!check(&list, &detail));
}

#[test]
fn matching_list_and_detail_cannot_make_invalid_metadata_valid() {
    for (field, value) in [
        ("name", json!("velnor-release-rust-12-2")),
        ("digest", json!("sha256:bad")),
        ("expired", json!(true)),
        ("size_in_bytes", json!(u64::MAX)),
    ] {
        let (mut list, mut detail) = valid();
        detail[field] = value;
        list["artifacts"][0] = detail.clone();
        assert!(!check(&list, &detail));
    }
    for field in ["id", "repository_id", "head_repository_id", "head_sha"] {
        let (mut list, mut detail) = valid();
        detail["workflow_run"][field] = if field == "head_sha" {
            json!("c".repeat(40))
        } else {
            json!(999)
        };
        list["artifacts"][0] = detail.clone();
        assert!(!check(&list, &detail));
    }
}

#[test]
fn receipt_artifact_requires_its_own_same_run_identity() {
    use velnor_actions_orchestrator::crate_release_tool_artifact::validate_receipt_artifact_metadata;
    let (mut list, mut detail) = valid();
    detail["name"] = json!("velnor-release-receipt-12-1");
    detail["size_in_bytes"] = json!(4096);
    list["artifacts"][0] = detail.clone();
    let check = |list: &Value, detail: &Value| {
        validate_receipt_artifact_metadata(
            list,
            detail,
            10,
            12,
            1,
            22,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
    };
    assert!(check(&list, &detail).is_ok());
    for (field, value) in [
        ("name", json!("velnor-release-rust-12-1")),
        ("expired", json!(true)),
        ("digest", json!("sha256:bad")),
    ] {
        let mut invalid = detail.clone();
        invalid[field] = value;
        let mut invalid_list = list.clone();
        invalid_list["artifacts"][0] = invalid.clone();
        assert!(check(&invalid_list, &invalid).is_err());
    }
}

#[test]
fn release_pr_artifact_requires_exact_same_run_identity() {
    use velnor_actions_orchestrator::crate_release_tool_artifact::validate_pr_preparation_artifact_metadata;
    let (mut list, mut detail) = valid();
    detail["name"] = json!("velnor-release-pr-preparation-12-1");
    detail["size_in_bytes"] = json!(4096);
    list["artifacts"][0] = detail.clone();
    let check = |list: &Value, detail: &Value| {
        validate_pr_preparation_artifact_metadata(
            list,
            detail,
            10,
            12,
            1,
            22,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
    };
    assert!(check(&list, &detail).is_ok());
    for (field, value) in [
        ("name", json!("velnor-release-pr-preparation-12-2")),
        ("expired", json!(true)),
        ("digest", json!("sha256:bad")),
        ("size_in_bytes", json!(u64::MAX)),
    ] {
        let mut invalid = detail.clone();
        invalid[field] = value;
        let mut invalid_list = list.clone();
        invalid_list["artifacts"][0] = invalid.clone();
        assert!(check(&invalid_list, &invalid).is_err());
    }
    for field in ["id", "repository_id", "head_repository_id", "head_sha"] {
        let mut invalid = detail.clone();
        invalid["workflow_run"][field] = if field == "head_sha" {
            json!("c".repeat(40))
        } else {
            json!(999)
        };
        let mut invalid_list = list.clone();
        invalid_list["artifacts"][0] = invalid.clone();
        assert!(check(&invalid_list, &invalid).is_err());
    }
    list["total_count"] = json!(2);
    assert!(check(&list, &detail).is_err());
}

#[test]
fn release_pr_archive_requires_one_regular_bounded_entry() -> Result<(), Box<dyn std::error::Error>>
{
    use std::io::Write;
    use velnor_actions_orchestrator::crate_release_tool_artifact::read_pr_preparation_archive;
    use zip::{ZipWriter, write::SimpleFileOptions};
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("candidate.zip");
    let make = |entries: &[(&str, &[u8])]| -> Result<(), Box<dyn std::error::Error>> {
        let mut zip = ZipWriter::new(std::fs::File::create(&path)?);
        for (name, data) in entries {
            zip.start_file(*name, SimpleFileOptions::default())?;
            zip.write_all(data)?;
        }
        zip.finish()?;
        Ok(())
    };
    make(&[("velnor-release-pr-preparation.json", b"{}")])?;
    assert_eq!(
        read_pr_preparation_archive(&std::fs::File::open(&path)?)?,
        b"{}"
    );
    let mut zip = ZipWriter::new(std::fs::File::create(&path)?);
    zip.start_file(
        "velnor-release-pr-preparation.json",
        SimpleFileOptions::default().large_file(true),
    )?;
    zip.write_all(b"{}")?;
    zip.finish()?;
    assert_eq!(
        read_pr_preparation_archive(&std::fs::File::open(&path)?)?,
        b"{}"
    );
    for entries in [
        vec![("other.json", &b"{}"[..])],
        vec![("../velnor-release-pr-preparation.json", &b"{}"[..])],
        vec![
            ("velnor-release-pr-preparation.json", &b"{}"[..]),
            ("extra", &b"x"[..]),
        ],
    ] {
        make(&entries)?;
        assert!(read_pr_preparation_archive(&std::fs::File::open(&path)?).is_err());
    }
    let original = b"xelnor-release-pr-preparation.json";
    let replacement = b"velnor-release-pr-preparation.json";
    make(&[
        ("velnor-release-pr-preparation.json", b"{}"),
        ("xelnor-release-pr-preparation.json", b"{}"),
    ])?;
    let mut bytes = std::fs::read(&path)?;
    let positions: Vec<_> = bytes
        .windows(original.len())
        .enumerate()
        .filter_map(|(index, value)| (value == original).then_some(index))
        .collect();
    assert_eq!(positions.len(), 2);
    for position in positions {
        bytes[position..position + replacement.len()].copy_from_slice(replacement);
    }
    std::fs::write(&path, bytes)?;
    assert!(read_pr_preparation_archive(&std::fs::File::open(&path)?).is_err());
    make(&[("velnor-release-pr-preparation.json", b"{}")])?;
    let mut bytes = std::fs::read(&path)?;
    let position = bytes
        .windows(replacement.len())
        .position(|value| value == replacement)
        .ok_or("filename")?;
    bytes[position] = b'x';
    std::fs::write(&path, bytes)?;
    assert!(read_pr_preparation_archive(&std::fs::File::open(&path)?).is_err());
    Ok(())
}

#[test]
fn receipt_archive_requires_one_regular_bounded_entry() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    use velnor_actions_orchestrator::crate_release_tool_artifact::read_receipt_archive;
    use zip::{ZipWriter, write::SimpleFileOptions};
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("receipt.zip");
    let make = |entries: &[(&str, &[u8])]| -> Result<(), Box<dyn std::error::Error>> {
        let mut zip = ZipWriter::new(std::fs::File::create(&path)?);
        for (name, data) in entries {
            zip.start_file(*name, SimpleFileOptions::default())?;
            zip.write_all(data)?;
        }
        zip.finish()?;
        Ok(())
    };
    make(&[("velnor-crate-release-receipt.json", b"{}")])?;
    assert_eq!(read_receipt_archive(&std::fs::File::open(&path)?)?, b"{}");
    for entries in [
        vec![("other.json", &b"{}"[..])],
        vec![("../velnor-crate-release-receipt.json", &b"{}"[..])],
        vec![
            ("velnor-crate-release-receipt.json", &b"{}"[..]),
            ("other", &b"x"[..]),
        ],
    ] {
        make(&entries)?;
        assert!(read_receipt_archive(&std::fs::File::open(&path)?).is_err());
    }
    make(&[
        ("velnor-crate-release-receipt.json", b"{}"),
        ("xelnor-crate-release-receipt.json", b"{}"),
    ])?;
    let original = b"xelnor-crate-release-receipt.json";
    let replacement = b"velnor-crate-release-receipt.json";
    let mut bytes = std::fs::read(&path)?;
    let positions: Vec<_> = bytes
        .windows(original.len())
        .enumerate()
        .filter_map(|(index, value)| (value == original).then_some(index))
        .collect();
    assert_eq!(positions.len(), 2);
    for position in positions {
        bytes[position..position + replacement.len()].copy_from_slice(replacement);
    }
    std::fs::write(&path, bytes)?;
    assert!(read_receipt_archive(&std::fs::File::open(&path)?).is_err());
    Ok(())
}

#[cfg(unix)]
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
    )
}
