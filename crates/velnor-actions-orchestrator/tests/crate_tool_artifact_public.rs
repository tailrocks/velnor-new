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
#[path = "crate_tool_artifact_public/toolchain.rs"]
mod toolchain;
