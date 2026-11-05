// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{io::Write, path::Path};
use velnor_actions_mise::CheckDeadline;

#[derive(Debug, Clone, Copy)]
pub struct HistoricalManifestRequest<'a> {
    pub manifest: &'a [u8],
    pub bundle: &'a [u8],
    pub version: &'a str,
    pub source_sha: &'a str,
    pub manifest_sha256: &'a str,
}

#[derive(Debug)]
pub struct HistoricalManifest {
    bytes: Vec<u8>,
    pub source_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
}

impl HistoricalManifest {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

pub fn verify_historical_manifest(
    request: &HistoricalManifestRequest<'_>,
    deadline: CheckDeadline,
) -> Result<HistoricalManifest, OrchestratorError> {
    use crate::package_release_signer::{
        SdkManifestAttestationExpectation, verify_published_manifest_attestation,
    };
    if request.manifest.is_empty()
        || request.manifest.len() > 1024 * 1024
        || request.bundle.is_empty()
        || request.bundle.len() > 4 * 1024 * 1024
        || crate::generator::sha256_hex(request.manifest) != request.manifest_sha256
    {
        return Err(invalid());
    }
    let raw = std::str::from_utf8(request.manifest).map_err(|_| invalid())?;
    let parsed =
        velnor_actions_workflow_renderer::release_helpers::validate_consumer_manifest_for_version(
            velnor_actions_contract::ReleaseTarget::LinuxX86_64,
            request.version,
            Some(raw),
        )
        .map_err(|_| invalid())?;
    if parsed.commit != request.source_sha {
        return Err(invalid());
    }
    let dir = tempfile::tempdir().map_err(|_| invalid())?;
    let manifest = write_asset(dir.path(), "release-manifest.json", request.manifest)?;
    let bundle = write_asset(
        dir.path(),
        "release-manifest.json.intoto.jsonl",
        request.bundle,
    )?;
    let attestation = verify_published_manifest_attestation(
        &manifest,
        &bundle,
        &SdkManifestAttestationExpectation {
            repository: "tailrocks/velnor-new",
            repository_id: 1_390_620_900,
            owner_id: 78_806_509,
            source_sha: request.source_sha,
            manifest_sha256: request.manifest_sha256,
        },
        deadline,
    )?;
    if attestation.source_sha != request.source_sha
        || attestation.workflow_sha != request.source_sha
        || attestation.event != "workflow_dispatch"
    {
        return Err(invalid());
    }
    crate::package_release_publish::verify_public_sdk_release_producer(
        &crate::package_release_publish::PublicSdkProducerSelection {
            repository: "tailrocks/velnor-new".to_owned(),
            repository_id: 1_390_620_900,
            source_sha: request.source_sha.to_owned(),
            run_id: attestation.run_id,
            run_attempt: attestation.run_attempt,
        },
        deadline,
    )?;
    Ok(HistoricalManifest {
        bytes: request.manifest.to_vec(),
        source_sha: attestation.source_sha,
        run_id: attestation.run_id,
        run_attempt: attestation.run_attempt,
    })
}

fn write_asset(
    root: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, OrchestratorError> {
    let path = root.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| invalid())?;
    file.write_all(bytes).map_err(|_| invalid())?;
    Ok(path)
}

fn invalid() -> OrchestratorError {
    internal("qualification_historical_manifest_invalid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    #[ignore = "requires signed public release assets, pinned trusted root, and GitHub read access"]
    fn authenticates_real_historical_manifest_and_rejects_mutations()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::var_os("VELNOR_ATTESTATION_TEST_DIR").ok_or("missing asset dir")?;
        let manifest = std::fs::read(Path::new(&dir).join("release-manifest.json"))?;
        let bundle = std::fs::read(Path::new(&dir).join("release-manifest.json.intoto.jsonl"))?;
        let expected = HistoricalManifestRequest {
            manifest: &manifest,
            bundle: &bundle,
            version: "0.1.73",
            source_sha: "82ad7e9f788718855469444a3a02ea79ba0fe087",
            manifest_sha256: "8e71b846acd375086fcb9693e567985d5a48b902e2e4b84400724ca6daad32d9",
        };
        let deadline = || CheckDeadline::after(Duration::from_secs(180));
        let actual = verify_historical_manifest(&expected, deadline()?)?;
        assert_eq!(actual.bytes(), manifest);
        let mut forged = manifest.clone();
        forged.push(b' ');
        assert!(
            verify_historical_manifest(
                &HistoricalManifestRequest {
                    manifest: &forged,
                    ..expected
                },
                deadline()?
            )
            .is_err()
        );
        let mut broken: serde_json::Value = serde_json::from_slice(&bundle)?;
        let signature = broken["dsseEnvelope"]["signatures"][0]["sig"]
            .as_str()
            .ok_or("signature")?;
        let mut altered = signature.as_bytes().to_vec();
        altered[0] = if altered[0] == b'A' { b'B' } else { b'A' };
        broken["dsseEnvelope"]["signatures"][0]["sig"] = String::from_utf8(altered)?.into();
        let broken = serde_json::to_vec(&broken)?;
        assert!(
            verify_historical_manifest(
                &HistoricalManifestRequest {
                    bundle: &broken,
                    ..expected
                },
                deadline()?
            )
            .is_err()
        );
        Ok(())
    }
}
