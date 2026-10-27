// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_producer_job_kept(text: &str) -> Result<(), Box<dyn Error>> {
    let build = job_section(
        text,
        "build-image-candidate",
        Some("attest-image-candidate"),
    )?;
    require(build, CHECKOUT_USES, "producer checkout")?;
    require(
        build,
        "Build exact-source native image maintenance",
        "producer source build",
    )?;
    require(
        build,
        "image-maintenance build-job-candidate",
        "producer build command",
    )?;
    require(
        build,
        "$VELNOR_NATIVE_IMAGE_ACTIONS",
        "producer native binary",
    )?;
    forbid(build, "publish.py", "producer authority")?;
    Ok(())
}

pub(crate) fn check_authority_job_data_only(
    section: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    require(section, STEP_NAME, label)?;
    require(section, BODY_DIRECTORY, label)?;
    require(section, DOWNLOAD_ARTIFACT_USES, label)?;
    require(section, "artifact-ids", label)?;
    for kept in [
        "actions/checkout",
        "Use the source commit selected by GitHub.",
        CHECKOUT_USES,
        "image-maintenance",
        "$VELNOR_NATIVE_IMAGE_ACTIONS",
        "VELNOR_NATIVE_IMAGE_ACTIONS",
        "Build exact-source",
        "native image maintenance",
        "python3 images/job",
        "publish.py --root",
        "GH_TOKEN",
        "github.token",
        "actions/cache",
        "cache-save",
        "cache-restore",
        "SCCACHE",
        "CARGO_INCREMENTAL",
        "cargo install",
    ] {
        forbid(section, kept, label)?;
    }
    Ok(())
}

pub(crate) fn check_attest_data_only(text: &str) -> Result<(), Box<dyn Error>> {
    let attest = job_section(text, "attest-image-candidate", Some("publish-job-image"))?;
    check_authority_job_data_only(attest, "attestation job")?;
    require(attest, VERIFY_STEP_NAME, "attestation verify")?;
    require(
        attest,
        ATTEST_BUILD_PROVENANCE_USES,
        "attestation provenance",
    )?;
    require(attest, "VELNOR_IMAGE_VERIFY_EOF", "attestation program")?;
    require(attest, "check_record_binding", "attestation program")?;
    require(attest, "verify_oci_archive", "attestation program")?;
    require(attest, "validate_record", "attestation program")?;
    require_order(attest, STEP_NAME, VERIFY_STEP_NAME, "attestation order")?;
    require_order(
        attest,
        VERIFY_STEP_NAME,
        ATTEST_BUILD_PROVENANCE_USES,
        "attestation order",
    )?;
    Ok(())
}

pub(crate) fn check_publish_data_only(text: &str) -> Result<(), Box<dyn Error>> {
    let publish = job_section(text, "publish-job-image", None)?;
    check_authority_job_data_only(publish, "publication job")?;
    require(publish, EVIDENCE_STEP_NAME, "publication evidence")?;
    require(publish, "VELNOR_IMAGE_EVIDENCE_EOF", "publication program")?;
    require(publish, "build_evidence", "publication program")?;
    require(publish, "read_manifest_bytes", "publication program")?;
    require(publish, "check_record_binding", "publication program")?;
    require(publish, "publish_image", "publication upload")?;
    require(publish, "fetch_registry_token", "publication upload")?;
    require(publish, "extract_blob", "publication upload")?;
    require(
        publish,
        "record[\\\"source\\\"]",
        "publication source binding",
    )?;
    require(publish, "VELNOR_REGISTRY_TOKEN", "publication upload")?;
    require(publish, "VELNOR_ARTIFACT_ID", "publication identity")?;
    require(
        publish,
        "Upload job image publication evidence",
        "publication evidence upload",
    )?;
    require(publish, "workflow_dispatch", "publication gate")?;
    require(publish, "refs/heads/main", "publication gate")?;
    require(publish, UPLOAD_ARTIFACT_USES, "publication upload")?;
    require_order(publish, STEP_NAME, EVIDENCE_STEP_NAME, "publication order")?;
    require_order(
        publish,
        EVIDENCE_STEP_NAME,
        "Upload job image publication evidence",
        "publication order",
    )?;
    Ok(())
}

pub(crate) fn check_shared_counts(text: &str) -> Result<(), Box<dyn Error>> {
    require_count(text, CHECKOUT_USES, 1, "checkout jobs")?;
    require_count(text, STEP_NAME, 2, "shared body step")?;
    require_count(text, BODY_DIRECTORY, 2, "shared body directory")?;
    require_count(
        text,
        "authority=\\\"$VELNOR_IMAGE_AUTHORITY_DIR\\\"",
        2,
        "shared body binding",
    )?;
    require_count(
        text,
        "VELNOR_IMAGE_AUTHORITY_DIR=%s",
        2,
        "shared body export",
    )?;
    require_count(text, END_MARKER, 24, "shared body files")?;
    require_count(text, VERIFY_MARKER, 2, "verify program")?;
    require_count(text, EVIDENCE_MARKER, 2, "evidence program")?;
    require_count(text, VERIFY_STEP_NAME, 1, "verify step")?;
    require_count(text, EVIDENCE_STEP_NAME, 1, "evidence step")?;
    for kept in [
        "python3 images/job/ubuntu-26.04/candidate/publish.py --root",
        "VELNOR_CANDIDATE_ARTIFACT_ID",
        "Do a check of source and archive identity",
    ] {
        forbid(text, kept, "removed source execution")?;
    }
    Ok(())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(crate) fn tar_entry(name: &str, data: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    let size_field = format!("{:011o}\0", data.len());
    header[124..136].copy_from_slice(size_field.as_bytes());
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let checksum_field = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_field.as_bytes());
    let mut entry = header.to_vec();
    entry.extend_from_slice(data);
    while entry.len() % 512 != 0 {
        entry.push(0);
    }
    entry
}

pub(crate) fn tar_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, data) in entries {
        archive.extend_from_slice(&tar_entry(name, data));
    }
    archive.extend_from_slice(&[0u8; 1024]);
    archive
}
