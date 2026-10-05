//! Hostile archive rejection for the fixed receipt ZIP reader.

use std::io::{Cursor, Write};

use velnor_actions_contract::{
    QUALIFICATION_CACHE_RECEIPT_ARTIFACT, QUALIFICATION_CACHE_RECEIPT_FILENAME,
    QualificationCacheArtifact,
};
use zip::write::SimpleFileOptions;

use super::{receipt_bytes, sha256};

#[test]
fn accepts_one_digest_verified_receipt_file() -> Result<(), String> {
    let bytes = zip_with(&[(
        QUALIFICATION_CACHE_RECEIPT_FILENAME,
        b"{\"schema\":1}" as &[u8],
        0o100_644,
    )])?;
    assert_eq!(
        receipt_bytes(&bytes, &artifact(&bytes)).map_err(|err| err.to_string())?,
        b"{\"schema\":1}"
    );
    Ok(())
}

#[test]
fn rejects_wrong_digest_extra_members_and_wrong_names() -> Result<(), String> {
    let exact = zip_with(&[(QUALIFICATION_CACHE_RECEIPT_FILENAME, b"receipt", 0o100_644)])?;
    let mut forged = artifact(&exact);
    forged.digest = "sha256:deadbeef".to_owned();
    assert!(receipt_bytes(&exact, &forged).is_err());

    let extra = zip_with(&[
        (QUALIFICATION_CACHE_RECEIPT_FILENAME, b"receipt", 0o100_644),
        ("extra.json", b"extra", 0o100_644),
    ])?;
    assert!(receipt_bytes(&extra, &artifact(&extra)).is_err());

    let renamed = zip_with(&[(
        "nested/qualification-cache-receipt.json",
        b"receipt",
        0o100_644,
    )])?;
    assert!(receipt_bytes(&renamed, &artifact(&renamed)).is_err());
    Ok(())
}

#[test]
fn rejects_a_symlinked_receipt_member() -> Result<(), String> {
    let mut bytes = zip_with(&[(QUALIFICATION_CACHE_RECEIPT_FILENAME, b"receipt", 0o777)])?;
    mark_first_central_member_symlink(&mut bytes)?;
    assert!(receipt_bytes(&bytes, &artifact(&bytes)).is_err());
    Ok(())
}

fn mark_first_central_member_symlink(bytes: &mut [u8]) -> Result<(), String> {
    let signature = b"PK\x01\x02";
    let offset = bytes
        .windows(signature.len())
        .position(|window| window == signature)
        .ok_or_else(|| "central directory not found".to_owned())?;
    let creator_system = offset + 5;
    let external_attributes = offset + 38;
    bytes[creator_system] = 3;
    bytes[external_attributes..external_attributes + 4]
        .copy_from_slice(&(0o120_777_u32 << 16).to_le_bytes());
    Ok(())
}

fn artifact(bytes: &[u8]) -> QualificationCacheArtifact {
    QualificationCacheArtifact {
        id: 7,
        name: QUALIFICATION_CACHE_RECEIPT_ARTIFACT.to_owned(),
        digest: sha256(bytes),
        size_bytes: bytes.len() as u64,
        expired: false,
        workflow_run_id: 11,
        workflow_head_branch: "main".to_owned(),
        workflow_head_sha: "a".repeat(40),
    }
}

fn zip_with(members: &[(&str, &[u8], u32)]) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, mode) in members {
        let options = SimpleFileOptions::default().unix_permissions(*mode);
        zip.start_file(name, options)
            .map_err(|err| err.to_string())?;
        zip.write_all(bytes).map_err(|err| err.to_string())?;
    }
    zip.finish()
        .map(Cursor::into_inner)
        .map_err(|err| err.to_string())
}
