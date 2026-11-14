// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::{error::Error, fs, path::PathBuf, process::Command};
use velnor_actions_mise::{
    COSIGN_COMMAND_OUTPUT_LIMIT, COSIGN_COMMAND_TIMEOUT, CosignCertificateIdentity, PinnedCosign,
};

const ENVELOPE: &[u8] = include_bytes!("fixtures/mac-proof/envelope.json");
const BUNDLE: &[u8] = include_bytes!("fixtures/mac-proof/envelope.sigstore.json");
const TRUSTED_ROOT: &[u8] = include_bytes!("../src/image_maintenance/sigstore-trusted-root.json");
const SHA: &str = "ab624e88dc4e3577d2b76450da34d72c1f9db623";

#[test]
fn historical_signed_envelope_authenticates_only_unchanged_bytes_and_identity()
-> Result<(), Box<dyn Error>> {
    let cosign = PinnedCosign::from_verified_path(&qualified_cosign()?)?;
    let dir = tempfile::tempdir()?;
    let subject = dir.path().join("envelope.json");
    let bundle = dir.path().join("bundle.json");
    let root = dir.path().join("trusted-root.json");
    fs::write(&subject, ENVELOPE)?;
    fs::write(&bundle, BUNDLE)?;
    fs::write(&root, TRUSTED_ROOT)?;
    let verify = |identity: &CosignCertificateIdentity| -> Result<bool, Box<dyn Error>> {
        Ok(cosign
            .verify_blob_command_with_trusted_root(&subject, &bundle, identity, &root)?
            .run_bounded(COSIGN_COMMAND_OUTPUT_LIMIT, COSIGN_COMMAND_TIMEOUT)?
            .success)
    };
    let identity = |repository, workflow, reference, sha, trigger| {
        CosignCertificateIdentity::new(repository, workflow, reference, sha, trigger)
    };
    let valid = identity(
        "donbeave/velnor-mac-qualification",
        ".github/workflows/verifier-fixture.yml",
        "refs/heads/main",
        SHA,
        "workflow_dispatch",
    )?;
    assert!(verify(&valid)?);
    let mut changed = ENVELOPE.to_vec();
    changed.push(b' ');
    fs::write(&subject, changed)?;
    assert!(!verify(&valid)?);
    fs::write(&subject, ENVELOPE)?;
    let mut changed_bundle: serde_json::Value = serde_json::from_slice(BUNDLE)?;
    changed_bundle["verificationMaterial"]["tlogEntries"] = serde_json::json!([]);
    fs::write(&bundle, serde_json::to_vec(&changed_bundle)?)?;
    assert!(!verify(&valid)?);
    fs::write(&bundle, BUNDLE)?;
    for changed in [
        identity(
            "donbeave/other",
            ".github/workflows/verifier-fixture.yml",
            "refs/heads/main",
            SHA,
            "workflow_dispatch",
        )?,
        identity(
            "donbeave/velnor-mac-qualification",
            ".github/workflows/other.yml",
            "refs/heads/main",
            SHA,
            "workflow_dispatch",
        )?,
        identity(
            "donbeave/velnor-mac-qualification",
            ".github/workflows/verifier-fixture.yml",
            "refs/heads/other",
            SHA,
            "workflow_dispatch",
        )?,
        identity(
            "donbeave/velnor-mac-qualification",
            ".github/workflows/verifier-fixture.yml",
            "refs/heads/main",
            "0000000000000000000000000000000000000000",
            "workflow_dispatch",
        )?,
        identity(
            "donbeave/velnor-mac-qualification",
            ".github/workflows/verifier-fixture.yml",
            "refs/heads/main",
            SHA,
            "workflow_run",
        )?,
    ] {
        assert!(!verify(&changed)?);
    }
    Ok(())
}

fn qualified_cosign() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = std::env::var_os("VELNOR_TEST_COSIGN") {
        return Ok(PathBuf::from(path));
    }
    let result = Command::new("mise")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "which",
            "cosign",
            "--tool",
            "aqua:sigstore/cosign@3.1.4",
        ])
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_OFFLINE", "1")
        .output()?;
    assert!(
        result.status.success(),
        "mise could not resolve pinned Cosign"
    );
    let output = std::str::from_utf8(&result.stdout)?.trim();
    assert!(!output.contains(['\r', '\n']));
    Ok(PathBuf::from(output))
}
