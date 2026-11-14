// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use velnor_actions_contract::runner_image_manifest::{
    RunnerImageManifest,
    mac_attestation::{
        MAC_ATTESTATION_SCHEMA, MacQualificationAttestation, MacQualificationEvidence,
        parse_mac_qualification_attestation, receipt_matches_manifest,
    },
};
use velnor_actions_orchestrator::OrchestratorError;

pub(super) fn run(command: &crate::args::ImageMaintenanceCommand) -> Result<(), OrchestratorError> {
    let crate::args::ImageMaintenanceCommand::AssembleMacAttestation {
        context,
        receipt,
        manifest,
        runner_lock,
        generator_lock,
        sdk_evidence,
        output,
    } = command
    else {
        return Err(invalid());
    };
    assemble(
        context,
        receipt,
        manifest,
        runner_lock,
        generator_lock,
        sdk_evidence,
        output,
    )
}

fn invalid() -> OrchestratorError {
    OrchestratorError::Internal {
        problem: "mac_attestation_assembly_invalid".to_owned(),
    }
}

fn assemble(
    context: &Path,
    receipt: &Path,
    manifest: &Path,
    runner_lock: &Path,
    generator_lock: &Path,
    sdk_evidence: &Path,
    output: &Path,
) -> Result<(), OrchestratorError> {
    let context = read(context, 64 * 1024)?;
    let receipt = read(receipt, 1024 * 1024)?;
    let manifest_bytes = read(manifest, 1024 * 1024)?;
    let runner_lock = read(runner_lock, 64 * 1024)?;
    let generator_lock = read(generator_lock, 1024 * 1024)?;
    let sdk_manifest = read(sdk_evidence, 1024 * 1024)?;
    let mut context =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&context, 64 * 1024)
            .map_err(|_| invalid())?;
    let fields = context.as_object_mut().ok_or_else(invalid)?;
    if fields.len() != 3 {
        return Err(invalid());
    }
    let public_authority =
        serde_json::from_value(fields.remove("public_authority").ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    let companion = serde_json::from_value(fields.remove("companion").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let sdk =
        serde_json::from_value(fields.remove("sdk").ok_or_else(invalid)?).map_err(|_| invalid())?;
    let envelope = MacQualificationAttestation {
        schema: MAC_ATTESTATION_SCHEMA.to_owned(),
        public_authority,
        companion,
        sdk,
        qualification: MacQualificationEvidence {
            receipt_base64: STANDARD.encode(&receipt),
            native_receipt_sha256: hex(&receipt),
        },
    };
    let mut bytes = serde_json::to_vec(&envelope).map_err(|_| invalid())?;
    bytes.push(b'\n');
    let (_, parsed_receipt) = parse_mac_qualification_attestation(&bytes).map_err(|_| invalid())?;
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&manifest_bytes, 1024 * 1024)
            .map_err(|_| invalid())?;
    let manifest: RunnerImageManifest = serde_json::from_value(value).map_err(|_| invalid())?;
    if !receipt_matches_manifest(&parsed_receipt, &manifest, &manifest_bytes)
        || parsed_receipt.runner_lock_sha256 != hex(&runner_lock)
    {
        return Err(invalid());
    }
    verify_runner_lock(&manifest, &runner_lock)?;
    verify_sdk(&envelope, &generator_lock, &sdk_manifest)?;
    write_new(output, &bytes)
}

fn verify_runner_lock(
    manifest: &RunnerImageManifest,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    let table: toml::Table = toml::from_str(std::str::from_utf8(bytes).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    for (key, expected) in [
        ("runner_version", &manifest.versions.runner),
        ("docker_version", &manifest.versions.docker),
        ("buildx_version", &manifest.versions.buildx),
        ("compose_version", &manifest.versions.compose),
        ("node_version", &manifest.versions.node),
    ] {
        if table.get(key).and_then(toml::Value::as_str) != Some(expected.as_str()) {
            return Err(invalid());
        }
    }
    Ok(())
}

fn verify_sdk(
    envelope: &MacQualificationAttestation,
    lock: &[u8],
    manifest: &[u8],
) -> Result<(), OrchestratorError> {
    use velnor_actions_mise::catalog::lock::{
        parse_generator_lock, parse_release_manifest, verify_lock_against_manifest,
    };
    let lock = parse_generator_lock(std::str::from_utf8(lock).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let sdk = &envelope.sdk;
    match &sdk.authority {
        velnor_actions_contract::runner_image_manifest::mac_attestation::MacSdkAuthority::Stable {
            release_manifest_sha256,
        } => {
            let release = parse_release_manifest(std::str::from_utf8(manifest).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
            verify_lock_against_manifest(&lock, &release).map_err(|_| invalid())?;
            let binary = lock.binary_for_target(&sdk.target).ok_or_else(invalid)?;
            if lock.generator.binary != "velnor-actions"
                || sdk.version != lock.generator.version
                || sdk.source_commit != lock.generator.commit
                || sdk.asset_sha256 != binary.sha256
                || *release_manifest_sha256 != hex(manifest)
            {
                return Err(invalid());
            }
        }
        velnor_actions_contract::runner_image_manifest::mac_attestation::MacSdkAuthority::SourcePreview {
            receipt_sha256,
            bundle_sha256,
            release_id,
            ..
        } => {
            let preview = lock.runtime_preview.as_ref().ok_or_else(invalid)?;
            preview
                .validate_receipt_bytes(manifest, &envelope.public_authority.source_commit)
                .map_err(|_| invalid())?;
            if sdk.version != preview.receipt.version
                || sdk.source_commit != preview.receipt.source_sha
                || sdk.asset_sha256 != preview.receipt.targets.darwin.payload.sha256
                || *receipt_sha256 != preview.receipt_sha256
                || *bundle_sha256 != preview.bundle_sha256
                || *release_id != preview.receipt.release_id
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, OrchestratorError> {
    let file = File::open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(u64::try_from(limit).map_err(|_| invalid())? + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| invalid())?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| invalid())?;
    temporary.persist_noclobber(path).map_err(|_| invalid())?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(ALPHABET[usize::from(byte >> 4)]),
                char::from(ALPHABET[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
