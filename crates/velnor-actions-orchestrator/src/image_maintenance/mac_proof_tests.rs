// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{error::Error, io};
#[cfg(target_os = "linux")]
use std::{fs, time::Duration};

const ENVELOPE: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/envelope.json");
const BUNDLE: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/envelope.sigstore.json");
#[cfg(target_os = "linux")]
const MANIFEST: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/manifest.json");
#[cfg(target_os = "linux")]
const CONTEXT: &[u8] = include_bytes!("../../tests/fixtures/mac-proof/context.json");
const SHA: &str = "ab624e88dc4e3577d2b76450da34d72c1f9db623";

#[cfg(target_os = "linux")]
#[test]
fn hosted_signed_fixture_authenticates_only_the_bound_release() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tools = temp.path().join("velnor/tools");
    fs::create_dir_all(&tools)?;
    let cosign = qualified_cosign()?;
    fs::copy(cosign, tools.join("cosign-3.1.4"))?;
    let context: serde_json::Value = serde_json::from_slice(CONTEXT)?;
    let public: MacPublicAuthority = serde_json::from_value(required(
        context.get("public").cloned(),
        "fixture public authority is missing",
    )?)?;
    let sdk: MacSdkIdentity = serde_json::from_value(required(
        context.get("sdk").cloned(),
        "fixture SDK identity is missing",
    )?)?;
    let manifest: RunnerImageManifest = serde_json::from_slice(MANIFEST)?;
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
    let policy = MacPrivatePolicy {
        workflow_path: ".github/workflows/verifier-fixture.yml",
        workflow_ref: "refs/heads/main",
        workflow_sha: SHA,
        runner_id: 42,
    };
    assert!(verify_with_policy(&request, &policy).is_ok());
    assert!(verify_mac_qualification(&request).is_err());
    check_authority_rejections(&request, &policy);
    check_sdk_rejections(&request, &policy);
    check_proof_rejections(&request, &policy)?;
    Ok(())
}

fn required<T>(value: Option<T>, message: &'static str) -> Result<T, Box<dyn Error>> {
    value.ok_or_else(|| io::Error::other(message).into())
}

#[cfg(target_os = "linux")]
fn qualified_cosign() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let explicit = std::env::var_os("VELNOR_TEST_COSIGN").map(std::path::PathBuf::from);
    let cache = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .and_then(|home| {
            home.parent()
                .map(|parent| parent.join("tools/cosign-3.1.4"))
        })
        .filter(|path| path.is_file());
    let path = if let Some(path) = explicit.or(cache) {
        path
    } else {
        let result = std::process::Command::new("mise")
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
        assert!(
            !output.contains(['\r', '\n']),
            "mise returned more than one path"
        );
        std::path::PathBuf::from(output)
    };
    assert!(
        path.is_absolute() && path.is_file(),
        "pinned Cosign path must be an absolute regular file"
    );
    Ok(path)
}

#[cfg(target_os = "linux")]
fn check_authority_rejections(
    request: &VerifyMacQualificationRequest<'_>,
    policy: &MacPrivatePolicy<'_>,
) {
    let public = request.public.clone();
    let mut changed_public = public.clone();
    changed_public.release_run_id += 1;
    assert!(
        verify_with_policy(
            &VerifyMacQualificationRequest {
                public: &changed_public,
                ..*request
            },
            policy
        )
        .is_err()
    );
    changed_public = public.clone();
    changed_public.producer_artifact_id += 1;
    assert!(
        verify_with_policy(
            &VerifyMacQualificationRequest {
                public: &changed_public,
                ..*request
            },
            policy
        )
        .is_err()
    );
    changed_public = public.clone();
    changed_public.publisher_artifact_id += 1;
    assert!(
        verify_with_policy(
            &VerifyMacQualificationRequest {
                public: &changed_public,
                ..*request
            },
            policy
        )
        .is_err()
    );
    for changed in [
        MacPublicAuthority {
            source_commit: "0".repeat(40),
            ..public.clone()
        },
        MacPublicAuthority {
            release_run_attempt: public.release_run_attempt + 1,
            ..public.clone()
        },
        MacPublicAuthority {
            repository_id: public.repository_id + 1,
            ..public.clone()
        },
    ] {
        assert!(
            verify_with_policy(
                &VerifyMacQualificationRequest {
                    public: &changed,
                    ..*request
                },
                policy
            )
            .is_err()
        );
    }
}

#[cfg(target_os = "linux")]
fn check_sdk_rejections(
    request: &VerifyMacQualificationRequest<'_>,
    policy: &MacPrivatePolicy<'_>,
) {
    let sdk = request.sdk.clone();
    for changed in [
        MacSdkIdentity {
            repository: "other/repository".into(),
            ..sdk.clone()
        },
        MacSdkIdentity {
            version: "2.0.0".into(),
            ..sdk.clone()
        },
        MacSdkIdentity {
            source_commit: "0".repeat(40),
            ..sdk.clone()
        },
        MacSdkIdentity {
            target: "x86_64-unknown-linux-gnu".into(),
            ..sdk.clone()
        },
        MacSdkIdentity {
            authority: velnor_actions_contract::runner_image_manifest::mac_attestation::MacSdkAuthority::Stable {
                release_manifest_sha256: "0".repeat(64),
            },
            ..sdk.clone()
        },
    ] {
        assert!(
            verify_with_policy(
                &VerifyMacQualificationRequest {
                    sdk: &changed,
                    ..*request
                },
                policy
            )
            .is_err()
        );
    }
    for changed in [
        MacPrivatePolicy {
            workflow_path: ".github/workflows/other.yml",
            ..*policy
        },
        MacPrivatePolicy {
            workflow_ref: "refs/heads/other",
            ..*policy
        },
        MacPrivatePolicy {
            workflow_sha: "0000000000000000000000000000000000000000",
            ..*policy
        },
        MacPrivatePolicy {
            runner_id: 43,
            ..*policy
        },
    ] {
        assert!(verify_with_policy(request, &changed).is_err());
    }
    let mut changed_sdk = sdk.clone();
    changed_sdk.asset_sha256 = "0".repeat(64);
    assert!(
        verify_with_policy(
            &VerifyMacQualificationRequest {
                sdk: &changed_sdk,
                ..*request
            },
            policy
        )
        .is_err()
    );
}

#[cfg(target_os = "linux")]
fn check_proof_rejections(
    request: &VerifyMacQualificationRequest<'_>,
    policy: &MacPrivatePolicy<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut changed_manifest = MANIFEST.to_vec();
    changed_manifest.push(b' ');
    assert!(
        verify_with_policy(
            &VerifyMacQualificationRequest {
                manifest_bytes: &changed_manifest,
                ..*request
            },
            policy
        )
        .is_err()
    );
    let mut changed_bundle: serde_json::Value = serde_json::from_slice(BUNDLE)?;
    changed_bundle["verificationMaterial"]["tlogEntries"] = serde_json::json!([]);
    let changed_bundle = serde_json::to_vec(&changed_bundle)?;
    assert!(
        verify_with_policy(
            &VerifyMacQualificationRequest {
                bundle: &changed_bundle,
                ..*request
            },
            policy
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn verified_certificate_claims_reject_other_private_authorities_and_invocations()
-> Result<(), Box<dyn Error>> {
    let (envelope, _) = parse_mac_qualification_attestation(ENVELOPE)
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    let bundle: serde_json::Value = serde_json::from_slice(BUNDLE)?;
    let certificate = STANDARD.decode(required(
        bundle
            .pointer("/verificationMaterial/certificate/rawBytes")
            .and_then(serde_json::Value::as_str),
        "fixture certificate is missing",
    )?)?;
    let policy = MacPrivatePolicy {
        workflow_path: ".github/workflows/verifier-fixture.yml",
        workflow_ref: "refs/heads/main",
        workflow_sha: SHA,
        runner_id: 42,
    };
    let valid = Certificate::from_der(&certificate)
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    assert!(verify_certificate_claims(&valid, &envelope.companion, &policy).is_ok());
    for (from, to) in [
        ("1450917587", "1450917588"),
        ("139017", "139018"),
        ("github-hosted", "github-hosteD"),
        ("private", "privatE"),
        (
            "ab624e88dc4e3577d2b76450da34d72c1f9db623",
            "ab624e88dc4e3577d2b76450da34d72c1f9db624",
        ),
        ("42577919528", "42577919529"),
        ("refs/heads/main", "refs/heads/maIn"),
        ("workflow_dispatch", "workflow_dispatcH"),
        ("verifier-fixture.yml", "verifier-fixturE.yml"),
    ] {
        let mut changed = certificate.clone();
        let matches: Vec<_> = changed
            .windows(from.len())
            .enumerate()
            .filter_map(|(i, value)| (value == from.as_bytes()).then_some(i))
            .collect();
        assert_ne!(matches.len(), 0);
        for i in matches {
            changed[i..i + from.len()].copy_from_slice(to.as_bytes());
        }
        let changed = Certificate::from_der(&changed)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(
            verify_certificate_claims(&changed, &envelope.companion, &policy).is_err(),
            "accepted altered claim {from}"
        );
    }
    Ok(())
}
