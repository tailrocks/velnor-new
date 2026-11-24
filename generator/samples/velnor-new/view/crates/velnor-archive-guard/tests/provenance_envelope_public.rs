// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod family_oci_deadline;

use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use sha2::{Digest, Sha256};
use tempfile::tempdir;
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    encode_provenance_envelope, inspect_oci_candidate_file_with_config, parse_provenance_envelope,
    project_attested_oci_file, verify_provenance_envelope,
};

const ARCHIVE: &[u8] = include_bytes!("fixtures/buildkit-attested-probe.oci.tar");

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn envelope_binds_actual_held_candidate_and_rejects_substitutions() -> TestResult<()> {
    let directory = tempdir()?;
    let original_path = directory.path().join("original.tar");
    std::fs::write(&original_path, ARCHIVE)?;
    let original = File::open(original_path)?;
    let projected_path = directory.path().join("projected.tar");
    let mut candidate = new_file(&projected_path)?;
    let result = project_attested_oci_file(&original, &mut candidate)?;
    assert_eq!(
        verify_provenance_envelope(&result.provenance, &candidate)?.statement(),
        result.provenance.statement
    );
    let replacement_path = directory.path().join("replacement.tar");
    let altered = changed_candidate(&std::fs::read(&projected_path)?)?;
    std::fs::write(&replacement_path, altered)?;
    let replacement = File::open(&replacement_path)?;
    let other = inspect_oci_candidate_file_with_config(&replacement)?;
    assert_ne!(
        other.evidence.platform_manifest_digest,
        result.candidate.evidence.platform_manifest_digest
    );
    assert_eq!(
        other.evidence.platform_manifest_size_bytes,
        result.candidate.evidence.platform_manifest_size_bytes
    );
    assert_eq!(
        other.manifest_media_type,
        result.candidate.manifest_media_type
    );
    std::fs::rename(replacement_path, &projected_path)?;
    assert!(verify_provenance_envelope(&result.provenance, &candidate).is_ok());
    let replaced = File::open(&projected_path)?;
    assert!(
        verify_provenance_envelope(&result.provenance, &replaced).is_err(),
        "substituted projected candidate was accepted"
    );
    check_substitutions(&result.provenance, &candidate);
    assert!(
        family_oci_deadline::expire_during(|| verify_provenance_envelope(
            &result.provenance,
            &candidate
        ))?
        .is_err()
    );
    Ok(())
}

#[test]
fn held_candidate_recheck_rejects_same_size_layer_mutation() -> TestResult<()> {
    let directory = tempdir()?;
    let source_path = directory.path().join("source.tar");
    std::fs::write(&source_path, ARCHIVE)?;
    let source = File::open(source_path)?;
    let candidate_path = directory.path().join("candidate.tar");
    let mut candidate = new_file(&candidate_path)?;
    let projection = project_attested_oci_file(&source, &mut candidate)?;
    let inspected = verify_provenance_envelope(&projection.provenance, &candidate)?;
    inspected.revalidate(&candidate)?;
    let layer = inspected.candidate().evidence.layer_digests[0].trim_start_matches("sha256:");
    let mut reader = File::open(&candidate_path)?;
    let mut archive = tar::Archive::new(&mut reader);
    let mut offset = None;
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.path()?.to_string_lossy() == format!("blobs/sha256/{layer}") {
            offset = Some(entry.raw_file_position());
            break;
        }
    }
    let mut actor = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&candidate_path)?;
    actor.seek(SeekFrom::Start(offset.ok_or("layer missing")?))?;
    let mut byte = [0_u8; 1];
    actor.read_exact(&mut byte)?;
    byte[0] ^= 1;
    actor.seek(SeekFrom::Current(-1))?;
    actor.write_all(&byte)?;
    actor.sync_all()?;
    assert!(
        inspected.revalidate(&candidate).is_err(),
        "changed held archive was accepted"
    );
    Ok(())
}

#[test]
fn canonical_envelope_rejects_invalid_or_oversized_input() -> TestResult<()> {
    let directory = tempdir()?;
    let source_path = directory.path().join("source.tar");
    std::fs::write(&source_path, ARCHIVE)?;
    let source = File::open(source_path)?;
    let mut candidate = new_file(&directory.path().join("candidate.tar"))?;
    let provenance = project_attested_oci_file(&source, &mut candidate)?.provenance;
    let encoded = encode_provenance_envelope(&provenance)?;
    assert_eq!(parse_provenance_envelope(&encoded)?, provenance);
    let text = String::from_utf8(encoded)?;
    let duplicate = text.replacen(
        "{\"schema\":",
        "{\"schema\":\"velnor.runner-provenance-envelope.v1\",\"schema\":",
        1,
    );
    assert_ne!(duplicate, text);
    assert!(
        parse_provenance_envelope(duplicate.as_bytes()).is_err(),
        "duplicate key accepted"
    );
    let unknown = text.replacen("{\"schema\":", "{\"unknown\":0,\"schema\":", 1);
    assert!(
        parse_provenance_envelope(unknown.as_bytes()).is_err(),
        "unknown key accepted"
    );
    let malformed = text.replacen("\"original_index_b64\":\"", "\"original_index_b64\":\"!", 1);
    assert_ne!(malformed, text);
    assert!(
        parse_provenance_envelope(malformed.as_bytes()).is_err(),
        "invalid base64 accepted"
    );
    let noncanonical_base64 = text.replace(
        "\"attestation_config_b64\":\"e30=\"",
        "\"attestation_config_b64\":\"e30\"",
    );
    assert_ne!(noncanonical_base64, text);
    assert!(
        parse_provenance_envelope(noncanonical_base64.as_bytes()).is_err(),
        "noncanonical base64 accepted"
    );
    let oversized = text.replacen(
        "\"attestation_descriptor\":{",
        &format!(
            "\"attestation_descriptor\":{{\"0\":\"{}\",",
            "a".repeat(8 * 1024 * 1024)
        ),
        1,
    );
    assert!(
        parse_provenance_envelope(oversized.as_bytes()).is_err(),
        "oversized envelope accepted"
    );
    let mut oversized_value = provenance.clone();
    oversized_value.attestation_descriptor["0"] = serde_json::json!("a".repeat(8 * 1024 * 1024));
    assert!(
        encode_provenance_envelope(&oversized_value).is_err(),
        "oversized encoding accepted"
    );
    let noncanonical = format!("{text} ");
    assert!(
        parse_provenance_envelope(noncanonical.as_bytes()).is_err(),
        "noncanonical envelope accepted"
    );
    Ok(())
}

fn check_substitutions(
    original: &velnor_archive_guard::owned_archive_guard::oci_candidate::UntrustedProvenance,
    candidate: &File,
) {
    let mut changed_index = original.clone();
    changed_index.buildkit_index[0] ^= 1;
    assert!(verify_provenance_envelope(&changed_index, candidate).is_err());
    let mut changed_manifest = original.clone();
    changed_manifest.attestation_manifest[0] ^= 1;
    assert!(verify_provenance_envelope(&changed_manifest, candidate).is_err());
    let mut changed_statement = original.clone();
    changed_statement.statement[0] ^= 1;
    assert!(verify_provenance_envelope(&changed_statement, candidate).is_err());
    let mut changed_revision = original.clone();
    let marker = b"vcs:revision\":\"";
    let start = changed_revision
        .statement
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|offset| offset + marker.len());
    assert!(start.is_some(), "fixture has no source revision");
    if let Some(start) = start {
        let byte = &mut changed_revision.statement[start];
        assert!(byte.is_ascii_hexdigit());
        *byte = if *byte == b'0' { b'1' } else { b'0' };
        assert_eq!(changed_revision.statement.len(), original.statement.len());
        assert_eq!(
            changed_revision
                .statement
                .iter()
                .zip(&original.statement)
                .filter(|(a, b)| a != b)
                .count(),
            1
        );
        assert!(serde_json::from_slice::<serde_json::Value>(&changed_revision.statement).is_ok());
        assert!(
            verify_provenance_envelope(&changed_revision, candidate).is_err(),
            "same-length provenance substitution was accepted"
        );
    }
    let mut changed_config = original.clone();
    changed_config.attestation_config = b"[]".to_vec();
    assert!(verify_provenance_envelope(&changed_config, candidate).is_err());
    let mut changed_descriptor = original.clone();
    changed_descriptor.attestation_descriptor["digest"] = serde_json::json!(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert!(verify_provenance_envelope(&changed_descriptor, candidate).is_err());
}

fn new_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

fn changed_candidate(bytes: &[u8]) -> TestResult<Vec<u8>> {
    let mut files = read_files(bytes)?;
    let index_bytes = files.get("index.json").ok_or("missing index")?;
    let index: serde_json::Value = serde_json::from_slice(index_bytes)?;
    let manifest_hash = index["manifests"][0]["digest"]
        .as_str()
        .ok_or("missing manifest digest")?
        .strip_prefix("sha256:")
        .ok_or("manifest digest format")?
        .to_owned();
    let manifest_path = format!("blobs/sha256/{manifest_hash}");
    let manifest_bytes = files.get(&manifest_path).ok_or("missing manifest")?;
    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes)?;
    let config_hash = manifest["config"]["digest"]
        .as_str()
        .ok_or("missing config digest")?
        .strip_prefix("sha256:")
        .ok_or("config digest format")?
        .to_owned();
    let config_path = format!("blobs/sha256/{config_hash}");
    let config = files.remove(&config_path).ok_or("missing config")?;
    let changed = replace_once(&config, b"\"WorkingDir\":\"/\"", b"\"WorkingDir\":\"x\"")?;
    let changed_config_hash = sha256_hex(&changed);
    files.insert(format!("blobs/sha256/{changed_config_hash}"), changed);
    let manifest_bytes = files.remove(&manifest_path).ok_or("missing manifest")?;
    let changed_manifest = replace_once(
        &manifest_bytes,
        config_hash.as_bytes(),
        changed_config_hash.as_bytes(),
    )?;
    let changed_manifest_hash = sha256_hex(&changed_manifest);
    files.insert(
        format!("blobs/sha256/{changed_manifest_hash}"),
        changed_manifest,
    );
    let index_bytes = files.remove("index.json").ok_or("missing index")?;
    files.insert(
        "index.json".to_owned(),
        replace_once(
            &index_bytes,
            manifest_hash.as_bytes(),
            changed_manifest_hash.as_bytes(),
        )?,
    );
    write_files(files)
}

fn replace_once(bytes: &[u8], old: &[u8], new: &[u8]) -> TestResult<Vec<u8>> {
    if old.len() != new.len() || old.is_empty() {
        return Err("replacement length".into());
    }
    let positions = bytes
        .windows(old.len())
        .enumerate()
        .filter_map(|(index, window)| (window == old).then_some(index))
        .collect::<Vec<_>>();
    let [index] = positions.as_slice() else {
        return Err("replacement count".into());
    };
    let mut changed = bytes.to_vec();
    changed[*index..*index + old.len()].copy_from_slice(new);
    Ok(changed)
}

fn read_files(bytes: &[u8]) -> TestResult<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for item in tar::Archive::new(bytes).entries()? {
        let mut entry = item?;
        if entry.header().entry_type().is_file() {
            let name = entry.path()?.to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            if files.insert(name, bytes).is_some() {
                return Err("duplicate fixture entry".into());
            }
        }
    }
    Ok(files)
}

fn write_files(files: BTreeMap<String, Vec<u8>>) -> TestResult<Vec<u8>> {
    let mut result = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut result);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(u64::try_from(bytes.len())?);
            header.set_mode(0o600);
            header.set_cksum();
            archive.append_data(&mut header, name, bytes.as_slice())?;
        }
        archive.finish()?;
    }
    Ok(result)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
