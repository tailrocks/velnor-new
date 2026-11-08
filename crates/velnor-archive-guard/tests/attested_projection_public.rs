// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod family_oci_deadline;

use std::{
    cell::Cell,
    collections::BTreeMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tempfile::tempdir;
use velnor_actions_mise::CheckDeadline;
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    inspect_oci_candidate_file_with_config, project_attested_oci_file,
};

const ARCHIVE: &[u8] = include_bytes!("fixtures/buildkit-attested-probe.oci.tar");

#[test]
fn projection_reports_deadline_after_output_begins() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let source_path = directory.path().join("source.tar");
    std::fs::write(&source_path, ARCHIVE)?;
    let source = std::fs::File::open(source_path)?;
    let output_path = directory.path().join("output.tar");
    let mut output = new_file(&output_path)?;
    let canary = directory.path().join("foreign");
    std::fs::write(&canary, b"preserve")?;
    let start = Instant::now();
    let deadline = CheckDeadline::from_start(start, Duration::from_secs(1))?;
    let expired = Rc::new(Cell::new(false));
    let observed = Rc::clone(&expired);
    let watched = output_path.clone();
    let mut result = None;
    let scope = CheckDeadline::with_clock(
        move || {
            if std::fs::read(&watched).is_ok_and(|bytes| {
                bytes
                    .as_chunks::<512>()
                    .0
                    .iter()
                    .any(|block| block.starts_with(b"blobs/sha256/"))
            }) {
                observed.set(true);
            }
            if observed.get() {
                start + Duration::from_secs(2)
            } else {
                start
            }
        },
        || deadline.scope(|| result = Some(project_attested_oci_file(&source, &mut output))),
    );
    assert!(scope.is_err());
    assert!(expired.get());
    let error = result
        .ok_or("projection did not start")?
        .err()
        .ok_or("projection did not expire")?;
    assert!(error.is_deadline_exceeded(), "unexpected error: {error}");
    assert_eq!(std::fs::read(canary)?, b"preserve");
    Ok(())
}

#[test]
fn projection_preserves_held_authenticated_platform_and_rejects_detached_attestation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let source_path = directory.path().join("original.tar");
    std::fs::write(&source_path, ARCHIVE)?;
    let source = std::fs::File::open(&source_path)?;
    let replacement = directory.path().join("replacement.tar");
    std::fs::write(&replacement, mutated_archive(ARCHIVE, Change::Reference)?)?;
    std::fs::rename(replacement, &source_path)?;
    let mut output = new_file(&directory.path().join("projected.tar"))?;
    let result = project_attested_oci_file(&source, &mut output)?;
    assert_eq!(
        result.candidate.evidence.platform_manifest_digest,
        "sha256:c6640793db3f495ed825a041f271d706f8de6854faca4c018fe2deca58936f5b"
    );
    assert_eq!(
        result.candidate,
        inspect_oci_candidate_file_with_config(&output)?
    );
    assert!(
        !result.candidate.index_has_annotations,
        "projected archive can create a tag"
    );
    let statement: serde_json::Value = serde_json::from_slice(&result.provenance.statement)?;
    assert_eq!(statement["predicateType"], "https://slsa.dev/provenance/v1");
    let detached = std::fs::File::open(&source_path)?;
    let mut other = new_file(&directory.path().join("detached.tar"))?;
    assert!(
        project_attested_oci_file(&detached, &mut other).is_err(),
        "detached attestation was admitted"
    );
    let mut expired = new_file(&directory.path().join("expired.tar"))?;
    assert!(
        family_oci_deadline::expire_during(|| project_attested_oci_file(&source, &mut expired))?
            .is_err()
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum Change {
    Reference,
    Subject,
    MissingBlob,
    ExtraBlob,
    Layer,
}

#[test]
fn projection_rejects_corrupt_graphs_and_preserves_existing_output()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    for (name, change) in [
        ("subject", Change::Subject),
        ("missing", Change::MissingBlob),
        ("extra", Change::ExtraBlob),
        ("layer", Change::Layer),
    ] {
        let source_path = directory.path().join(format!("{name}.source"));
        std::fs::write(&source_path, mutated_archive(ARCHIVE, change)?)?;
        let source = std::fs::File::open(source_path)?;
        let mut output = new_file(&directory.path().join(format!("{name}.output")))?;
        assert!(
            project_attested_oci_file(&source, &mut output).is_err(),
            "corrupt {name} graph was admitted"
        );
    }
    let source_path = directory.path().join("clean.source");
    std::fs::write(&source_path, ARCHIVE)?;
    let source = std::fs::File::open(source_path)?;
    let output_path = directory.path().join("foreign.output");
    std::fs::write(&output_path, b"foreign canary")?;
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&output_path)?;
    assert!(project_attested_oci_file(&source, &mut output).is_err());
    assert_eq!(std::fs::read(output_path)?, b"foreign canary");
    Ok(())
}

fn new_file(path: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

fn mutated_archive(bytes: &[u8], change: Change) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut files = read_files(bytes)?;
    let mut index: serde_json::Value =
        serde_json::from_slice(files.get("index.json").ok_or("outer index")?)?;
    let old = index["manifests"][0]["digest"]
        .as_str()
        .ok_or("nested digest")?
        .trim_start_matches("sha256:");
    let mut nested: serde_json::Value = serde_json::from_slice(
        files
            .get(&format!("blobs/sha256/{old}"))
            .ok_or("nested bytes")?,
    )?;
    let descriptors = nested["manifests"].as_array_mut().ok_or("descriptors")?;
    let attestation = descriptors
        .iter_mut()
        .find(|entry| entry["platform"]["os"] == "unknown")
        .ok_or("attestation")?;
    match change {
        Change::Reference => {
            attestation["annotations"]["vnd.docker.reference.digest"] = serde_json::json!(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            );
        }
        Change::Subject => change_subject(attestation, &mut files)?,
        Change::MissingBlob => {
            let digest = attestation["digest"]
                .as_str()
                .ok_or("attestation digest")?
                .trim_start_matches("sha256:");
            files.remove(&format!("blobs/sha256/{digest}"));
        }
        Change::ExtraBlob => {
            let bytes = b"foreign blob".to_vec();
            files.insert(format!("blobs/sha256/{}", sha256_hex(&bytes)), bytes);
        }
        Change::Layer => {
            let platform = descriptors
                .iter()
                .find(|entry| entry["platform"]["os"] == "linux")
                .ok_or("platform")?;
            let digest = platform["digest"]
                .as_str()
                .ok_or("platform digest")?
                .trim_start_matches("sha256:");
            let manifest: serde_json::Value = serde_json::from_slice(
                files
                    .get(&format!("blobs/sha256/{digest}"))
                    .ok_or("platform bytes")?,
            )?;
            let layer = manifest["layers"][0]["digest"]
                .as_str()
                .ok_or("layer digest")?
                .trim_start_matches("sha256:");
            let bytes = files
                .get_mut(&format!("blobs/sha256/{layer}"))
                .ok_or("layer bytes")?;
            bytes[0] ^= 1;
        }
    }
    let nested_bytes = serde_json::to_vec(&nested)?;
    let digest = sha256_hex(&nested_bytes);
    files.remove(&format!("blobs/sha256/{old}"));
    files.insert(format!("blobs/sha256/{digest}"), nested_bytes.clone());
    index["manifests"][0]["digest"] = serde_json::json!(format!("sha256:{digest}"));
    index["manifests"][0]["size"] = serde_json::json!(nested_bytes.len());
    files.insert("index.json".to_owned(), serde_json::to_vec(&index)?);
    write_files(files)
}

fn change_subject(
    attestation: &mut serde_json::Value,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let old_attestation = attestation["digest"]
        .as_str()
        .ok_or("attestation digest")?
        .trim_start_matches("sha256:")
        .to_owned();
    let mut manifest: serde_json::Value = serde_json::from_slice(
        files
            .get(&format!("blobs/sha256/{old_attestation}"))
            .ok_or("attestation bytes")?,
    )?;
    manifest["subject"]["digest"] = serde_json::json!(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    let bytes = serde_json::to_vec(&manifest)?;
    let digest = sha256_hex(&bytes);
    files.remove(&format!("blobs/sha256/{old_attestation}"));
    files.insert(format!("blobs/sha256/{digest}"), bytes.clone());
    attestation["digest"] = serde_json::json!(format!("sha256:{digest}"));
    attestation["size"] = serde_json::json!(bytes.len());
    Ok(())
}

fn read_files(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    let mut files = BTreeMap::new();
    for item in tar::Archive::new(bytes).entries()? {
        let mut entry = item?;
        if entry.header().entry_type().is_file() {
            let path = entry.path()?.to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            files.insert(path, bytes);
        }
    }
    Ok(files)
}

fn write_files(files: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut result = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut result);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            archive.append_data(&mut header, name, bytes.as_slice())?;
        }
        archive.finish()?;
    }
    result.flush()?;
    Ok(result)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::new();
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
