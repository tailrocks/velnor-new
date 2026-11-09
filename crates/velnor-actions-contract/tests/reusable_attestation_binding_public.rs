// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use velnor_actions_contract::reusable_attestation_binding::{
    AttestedSource, ReusableAttestationBinding, WorkflowRevision,
};

const OUTPUT: &[u8] = include_bytes!("fixtures/reusable_attestation/gh-output.json");
const SUBJECT: &[u8] = include_bytes!("fixtures/reusable_attestation/subject.json");
const REVISION: &str = "ff0f3ce3e7f87c172dbfc3b4ff9640f82c5a6905";

fn expected() -> ReusableAttestationBinding {
    let workflow = |path: &str| WorkflowRevision {
        repository: "tailrocks/velnor-new".into(),
        path: path.into(),
        reference: "refs/heads/main".into(),
        revision: REVISION.into(),
    };
    let digest = Sha256::digest(SUBJECT)
        .iter()
        .flat_map(|byte| {
            [
                char::from(b"0123456789abcdef"[usize::from(byte >> 4)]),
                char::from(b"0123456789abcdef"[usize::from(byte & 15)]),
            ]
        })
        .collect();
    ReusableAttestationBinding {
        signer: workflow(".github/workflows/product-release-generator.yml"),
        caller: workflow(".github/workflows/product-release.yml"),
        source: AttestedSource {
            repository: "tailrocks/velnor-new".into(),
            repository_id: 1_390_620_900,
            owner_id: 78_806_509,
            git_ref: "refs/heads/main".into(),
            revision: REVISION.into(),
        },
        event: "workflow_dispatch".into(),
        run_id: 42_339_974_423,
        run_attempt: 1,
        subjects: std::collections::BTreeMap::from([
            (
                "velnor-actions-0.1.78-x86_64-unknown-linux-gnu".into(),
                "3caad6b07553a6840496b9a729bfaac86efa8071be2d32268b06a63d2c1aa9e7".into(),
            ),
            (
                "velnor-actions-0.1.78-x86_64-unknown-linux-gnu.sha256".into(),
                "227dfedc7759c2bb91e1d0fd170d5788d352a930dc8bd80ea6162c7de7337386".into(),
            ),
            (
                "velnor-actions-0.1.78-x86_64-unknown-linux-gnu.provenance.json".into(),
                digest,
            ),
        ]),
    }
}

#[test]
fn genuine_reusable_workflow_output_matches_separate_caller_and_signer() {
    assert!(expected().check_untrusted_output_shape(OUTPUT).is_ok());
}

#[test]
fn mismatched_certificate_and_statement_fields_reject() -> Result<(), Box<dyn std::error::Error>> {
    let base: serde_json::Value = serde_json::from_slice(OUTPUT)?;
    for path in [
        "/0/verificationResult/signature/certificate/issuer",
        "/0/verificationResult/signature/certificate/subjectAlternativeName",
        "/0/verificationResult/signature/certificate/buildSignerURI",
        "/0/verificationResult/signature/certificate/buildSignerDigest",
        "/0/verificationResult/signature/certificate/buildConfigURI",
        "/0/verificationResult/signature/certificate/buildConfigDigest",
        "/0/verificationResult/signature/certificate/sourceRepositoryURI",
        "/0/verificationResult/signature/certificate/sourceRepositoryDigest",
        "/0/verificationResult/signature/certificate/sourceRepositoryRef",
        "/0/verificationResult/signature/certificate/sourceRepositoryIdentifier",
        "/0/verificationResult/signature/certificate/sourceRepositoryOwnerIdentifier",
        "/0/verificationResult/signature/certificate/runInvocationURI",
        "/0/verificationResult/signature/certificate/buildTrigger",
        "/0/verificationResult/signature/certificate/runnerEnvironment",
        "/0/verificationResult/statement/predicateType",
        "/0/verificationResult/statement/subject/0/digest/sha256",
        "/0/verificationResult/statement/predicate/runDetails/builder/id",
        "/0/verificationResult/statement/predicate/runDetails/metadata/invocationId",
        "/0/verificationResult/statement/predicate/buildDefinition/externalParameters/workflow/path",
        "/0/verificationResult/statement/predicate/buildDefinition/resolvedDependencies/0/digest/gitCommit",
    ] {
        let mut value = base.clone();
        *value.pointer_mut(path).ok_or("fixture field absent")? = serde_json::json!("substituted");
        assert!(
            expected()
                .check_untrusted_output_shape(&serde_json::to_vec(&value)?)
                .is_err(),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn structural_matching_does_not_conflate_three_revisions() -> Result<(), Box<dyn std::error::Error>>
{
    let mut value: serde_json::Value = serde_json::from_slice(OUTPUT)?;
    let mut binding = expected();
    binding.signer.revision = "a".repeat(40);
    binding.caller.revision = "b".repeat(40);
    value[0]["verificationResult"]["signature"]["certificate"]["buildSignerDigest"] =
        serde_json::json!(binding.signer.revision);
    value[0]["verificationResult"]["signature"]["certificate"]["buildConfigDigest"] =
        serde_json::json!(binding.caller.revision);
    assert!(
        binding
            .check_untrusted_output_shape(&serde_json::to_vec(&value)?)
            .is_ok()
    );
    assert!(
        expected()
            .check_untrusted_output_shape(&serde_json::to_vec(&value)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn malformed_oversized_duplicate_and_multiple_outputs_reject()
-> Result<(), Box<dyn std::error::Error>> {
    for bytes in [
        b"not json".as_slice(),
        b"[]",
        b"[null]",
        b"[{\"verificationResult\":{},\"verificationResult\":{}}]",
    ] {
        assert!(expected().check_untrusted_output_shape(bytes).is_err());
    }
    assert!(
        expected()
            .check_untrusted_output_shape(&vec![b' '; 128 * 1024 + 1])
            .is_err()
    );
    let output: serde_json::Value = serde_json::from_slice(OUTPUT)?;
    let duplicate = serde_json::json!([output[0], output[0]]);
    assert!(
        expected()
            .check_untrusted_output_shape(&serde_json::to_vec(&duplicate)?)
            .is_err()
    );
    let mut binding = expected();
    binding.subjects.insert("invalid".into(), "invalid".into());
    assert!(binding.check_untrusted_output_shape(OUTPUT).is_err());
    Ok(())
}

#[test]
fn structural_sha_pinned_callee_requires_matching_canonical_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut value: serde_json::Value = serde_json::from_slice(OUTPUT)?;
    let mut binding = expected();
    binding.signer.reference = binding.signer.revision.clone();
    let uri = format!(
        "https://github.com/{}/{}@{}",
        binding.signer.repository, binding.signer.path, binding.signer.reference
    );
    for field in ["subjectAlternativeName", "buildSignerURI"] {
        value[0]["verificationResult"]["signature"]["certificate"][field] = serde_json::json!(uri);
    }
    value[0]["verificationResult"]["statement"]["predicate"]["runDetails"]["builder"]["id"] =
        serde_json::json!(uri);
    let output = serde_json::to_vec(&value)?;
    assert!(binding.check_untrusted_output_shape(&output).is_ok());
    for reference in [
        "a".repeat(40),
        REVISION.to_uppercase(),
        REVISION[..39].into(),
    ] {
        binding.signer.reference = reference;
        let uri = format!(
            "https://github.com/{}/{}@{}",
            binding.signer.repository, binding.signer.path, binding.signer.reference
        );
        for field in ["subjectAlternativeName", "buildSignerURI"] {
            value[0]["verificationResult"]["signature"]["certificate"][field] =
                serde_json::json!(uri);
        }
        value[0]["verificationResult"]["statement"]["predicate"]["runDetails"]["builder"]["id"] =
            serde_json::json!(uri);
        assert!(
            binding
                .check_untrusted_output_shape(&serde_json::to_vec(&value)?)
                .is_err()
        );
    }
    binding = expected();
    binding.source.git_ref = binding.source.revision.clone();
    assert!(binding.check_untrusted_output_shape(OUTPUT).is_err());
    Ok(())
}
