// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{error::Error, io, time::Duration};
use x509_cert::der::{Encode, asn1::OctetString};

const ENVELOPE: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/envelope.json");
const BUNDLE: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/envelope.sigstore.json");
const MANIFEST: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/manifest.json");
const CONTEXT: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/context.json");
const SHA: &str = "ab624e88dc4e3577d2b76450da34d72c1f9db623";

#[test]
fn retired_signed_envelope_is_not_admitted() -> Result<(), Box<dyn Error>> {
    let context: serde_json::Value = serde_json::from_slice(CONTEXT)?;
    let public: MacPublicAuthority = serde_json::from_value(context["public"].clone())?;
    let sdk: MacSdkIdentity = serde_json::from_value(context["sdk"].clone())?;
    let manifest: RunnerImageManifest = serde_json::from_slice(MANIFEST)?;
    let temp = tempfile::tempdir()?;
    let request = VerifyMacQualificationRequest {
        envelope: ENVELOPE,
        bundle: BUNDLE,
        public: &public,
        sdk: &sdk,
        manifest_bytes: MANIFEST,
        manifest: &manifest,
        workflow_sha: SHA,
        runner_temp: temp.path(),
        deadline: CheckDeadline::after(Duration::from_secs(180))?,
    };
    assert!(verify_mac_qualification(&request).is_err());
    Ok(())
}

#[test]
fn unsigned_certificate_claim_specimen_requires_current_private_identity()
-> Result<(), Box<dyn Error>> {
    let envelope: velnor_actions_contract::runner_image_manifest::mac_attestation::MacQualificationAttestation = serde_json::from_slice(ENVELOPE)?;
    let policy = MacPrivatePolicy {
        workflow_path: ".github/workflows/verifier-fixture.yml",
        workflow_ref: "refs/heads/main",
        workflow_sha: SHA,
        runner_id: 42,
    };
    let historical = certificate_from_bundle(BUNDLE)?;
    assert!(verify_certificate_claims(&historical, &envelope.companion, &policy).is_err());
    let mut specimen = historical.clone();
    set_claim(&mut specimen, 15, "1451604567")?;
    assert!(verify_certificate_claims(&specimen, &envelope.companion, &policy).is_ok());
    for number in 8..=22 {
        let mut changed = specimen.clone();
        let extension = extension_mut(&mut changed, number)?;
        let original = Utf8StringRef::from_der(extension.extn_value.as_bytes())
            .map_err(|error| io::Error::other(format!("{error:?}")))?
            .as_str();
        let replacement = format!("{original}x");
        set_claim(&mut changed, number, &replacement)?;
        assert!(
            verify_certificate_claims(&changed, &envelope.companion, &policy).is_err(),
            "accepted altered claim {number}"
        );
    }
    let mut duplicate = specimen.clone();
    let ext = extension_mut(&mut duplicate, 15)?.clone();
    duplicate
        .tbs_certificate
        .extensions
        .as_mut()
        .ok_or("missing extensions")?
        .push(ext);
    assert!(verify_certificate_claims(&duplicate, &envelope.companion, &policy).is_err());
    let mut missing = specimen.clone();
    missing
        .tbs_certificate
        .extensions
        .as_mut()
        .ok_or("missing extensions")?
        .retain(|ext| ext.extn_id.to_string() != "1.3.6.1.4.1.57264.1.15");
    assert!(verify_certificate_claims(&missing, &envelope.companion, &policy).is_err());
    let mut malformed = specimen;
    extension_mut(&mut malformed, 15)?.extn_value =
        OctetString::new(vec![0xff]).map_err(|error| io::Error::other(format!("{error:?}")))?;
    assert!(verify_certificate_claims(&malformed, &envelope.companion, &policy).is_err());
    Ok(())
}

fn extension_mut(
    cert: &mut Certificate,
    number: u8,
) -> Result<&mut x509_cert::ext::Extension, Box<dyn Error>> {
    let oid = format!("1.3.6.1.4.1.57264.1.{number}");
    cert.tbs_certificate
        .extensions
        .as_mut()
        .and_then(|extensions| {
            extensions
                .iter_mut()
                .find(|extension| extension.extn_id.to_string() == oid)
        })
        .ok_or_else(|| io::Error::other("missing claim").into())
}

fn set_claim(cert: &mut Certificate, number: u8, value: &str) -> Result<(), Box<dyn Error>> {
    extension_mut(cert, number)?.extn_value = OctetString::new(
        Utf8StringRef::new(value)
            .and_then(|value| value.to_der())
            .map_err(|error| io::Error::other(format!("{error:?}")))?,
    )
    .map_err(|error| io::Error::other(format!("{error:?}")))?;
    Ok(())
}
