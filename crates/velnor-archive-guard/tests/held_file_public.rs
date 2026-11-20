// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod family_oci_deadline;

use std::error::Error;
use std::fs::{self, File};
use std::io::{Cursor, Seek, SeekFrom};
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    MAX_OCI_CANDIDATE_ARCHIVE_BYTES, inspect_oci_candidate_file_with_config,
    inspect_oci_candidate_with_config, materialize_oci_candidate, materialize_oci_candidate_file,
};

#[test]
fn materialization_uses_the_held_file_after_path_replacement() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("candidate.oci");
    let replacement = temp.path().join("replacement.oci");
    let first = archive("first")?;
    let second = archive("second")?;
    fs::write(&path, &first)?;
    fs::write(&replacement, &second)?;
    let mut held = File::open(&path)?;
    let first_evidence = inspect_oci_candidate_file_with_config(&held)?.evidence;
    fs::rename(&replacement, &path)?;
    let second_evidence = inspect_oci_candidate_with_config(&path)?.evidence;
    held.seek(SeekFrom::End(0))?;
    let first_output = temp.path().join("first-layout");
    let second_output = temp.path().join("second-layout");
    materialize_oci_candidate_file(&held, &first_output, &first_evidence)?;
    materialize_oci_candidate(&path, &second_output, &second_evidence)?;
    assert_eq!(
        fs::read(first_output.join("index.json"))?,
        index_bytes(&first)?
    );
    assert_eq!(
        fs::read(second_output.join("index.json"))?,
        index_bytes(&second)?
    );
    assert_ne!(
        first_evidence.platform_manifest_digest,
        second_evidence.platform_manifest_digest
    );
    let wrong = temp.path().join("wrong-layout");
    assert!(materialize_oci_candidate_file(&held, &wrong, &second_evidence).is_err());
    assert!(!wrong.exists());
    let expired = temp.path().join("expired-layout");
    assert!(
        family_oci_deadline::expire_during(|| materialize_oci_candidate_file(
            &held,
            &expired,
            &first_evidence
        ))?
        .is_err()
    );
    assert!(!expired.exists());
    Ok(())
}

fn index_bytes(archive: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    use std::io::Read;
    for entry in tar::Archive::new(Cursor::new(archive)).entries()? {
        let mut entry = entry?;
        if entry.path()? == Path::new("index.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }
    }
    Err("missing index".into())
}

#[test]
fn held_file_remains_the_inspected_archive_when_the_path_is_replaced() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("candidate.oci");
    let replacement = temp.path().join("replacement.oci");
    let first = archive("first")?;
    let second = archive("second")?;
    fs::write(&path, &first)?;
    fs::write(&replacement, &second)?;
    let mut held = File::open(&path)?;
    held.seek(SeekFrom::Start(123))?;
    fs::rename(&replacement, &path)?;
    let held_result = inspect_oci_candidate_file_with_config(&held)?;
    let path_result = inspect_oci_candidate_with_config(&path)?;
    assert_eq!(held_result.evidence.archive_sha256, sha256(&first));
    assert_eq!(path_result.evidence.archive_sha256, sha256(&second));
    assert_ne!(
        held_result.evidence.platform_manifest_digest,
        path_result.evidence.platform_manifest_digest
    );
    assert_ne!(
        held_result.evidence.image_configuration_digest,
        path_result.evidence.image_configuration_digest
    );
    assert_ne!(
        held_result.evidence.layer_digests,
        path_result.evidence.layer_digests
    );
    assert_eq!(held_result.configuration.user.as_deref(), Some("first"));
    assert_eq!(path_result.configuration.user.as_deref(), Some("second"));
    Ok(())
}

#[test]
fn held_file_rejects_nonregular_oversize_and_expired_input() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("candidate.oci");
    fs::write(&path, archive("candidate")?)?;
    let held = File::open(&path)?;
    let error =
        family_oci_deadline::expire_during(|| inspect_oci_candidate_file_with_config(&held))?
            .err()
            .ok_or("expired deadline was accepted")?;
    assert!(error.is_deadline_exceeded());
    let directory = File::open(temp.path())?;
    assert!(inspect_oci_candidate_file_with_config(&directory).is_err());
    let oversized_path = temp.path().join("oversized.oci");
    let oversized = File::create(&oversized_path)?;
    oversized.set_len(MAX_OCI_CANDIDATE_ARCHIVE_BYTES + 1)?;
    assert!(inspect_oci_candidate_file_with_config(&oversized).is_err());
    Ok(())
}

fn archive(name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let layers = [layer(name, "one")?, layer(name, "two")?];
    let layer_hashes: Vec<_> = layers.iter().map(|bytes| sha256(bytes)).collect();
    let config = serde_json::to_vec(
        &json!({"architecture":"amd64","os":"linux","config":{"User":name},"rootfs":{"type":"layers","diff_ids":layer_hashes.iter().map(|hash| format!("sha256:{hash}")).collect::<Vec<_>>()}}),
    )?;
    let config_hash = sha256(&config);
    let manifest = serde_json::to_vec(
        &json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":descriptor("application/vnd.oci.image.config.v1+json", &config_hash, config.len()),"layers":layers.iter().zip(&layer_hashes).map(|(bytes, hash)| descriptor("application/vnd.oci.image.layer.v1.tar", hash, bytes.len())).collect::<Vec<_>>() }),
    )?;
    let manifest_hash = sha256(&manifest);
    let index = serde_json::to_vec(
        &json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":format!("sha256:{manifest_hash}"),"size":manifest.len(),"platform":{"os":"linux","architecture":"amd64"}}]}),
    )?;
    let mut tar = tar::Builder::new(Vec::new());
    append(
        &mut tar,
        Path::new("oci-layout"),
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    )?;
    append(&mut tar, Path::new("index.json"), &index)?;
    append(
        &mut tar,
        Path::new(&format!("blobs/sha256/{manifest_hash}")),
        &manifest,
    )?;
    append(
        &mut tar,
        Path::new(&format!("blobs/sha256/{config_hash}")),
        &config,
    )?;
    for (bytes, hash) in layers.iter().zip(&layer_hashes) {
        append(&mut tar, Path::new(&format!("blobs/sha256/{hash}")), bytes)?;
    }
    tar.finish()?;
    Ok(tar.into_inner()?)
}

fn layer(name: &str, part: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut tar = tar::Builder::new(Vec::new());
    append(
        &mut tar,
        Path::new(&format!("{name}-{part}")),
        format!("{name}-{part}").as_bytes(),
    )?;
    tar.finish()?;
    Ok(tar.into_inner()?)
}

fn append(
    tar: &mut tar::Builder<Vec<u8>>,
    path: &Path,
    bytes: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len().try_into()?);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, path, Cursor::new(bytes))?;
    Ok(())
}

fn descriptor(media_type: &str, digest: &str, size: usize) -> serde_json::Value {
    json!({"mediaType":media_type,"digest":format!("sha256:{digest}"),"size":size})
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
