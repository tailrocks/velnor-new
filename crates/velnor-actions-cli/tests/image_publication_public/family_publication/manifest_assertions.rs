// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, fs};

use super::super::support::FamilyFixture;

pub(super) fn assert_manifest_matches_fixture(
    fixture: &FamilyFixture,
    bytes: &[u8],
) -> Result<(), Box<dyn Error>> {
    let manifest: Value = serde_json::from_slice(bytes)?;
    let _: velnor_actions_contract::runner_image_manifest::RunnerImageManifest =
        serde_json::from_slice(bytes)?;
    let state = fixture.state()?;
    assert_manifest_authority(&manifest);
    let mut lock = None;
    for (role, repository, record, envelope, context) in [
        (
            "runner",
            "ghcr.io/tailrocks/velnor-runner",
            "runner-image-publication.json",
            "runner-image-provenance.json",
            "images/runner/ubuntu-26.04",
        ),
        (
            "dind",
            "ghcr.io/tailrocks/velnor-dind",
            "dind-image-publication.json",
            "dind-image-provenance.json",
            "images/dind",
        ),
        (
            "resource_probe",
            "ghcr.io/tailrocks/velnor-resource-probe",
            "resource-probe-image-publication.json",
            "resource-probe-image-provenance.json",
            "images/resource-probe",
        ),
    ] {
        assert_image_entry(
            fixture,
            &manifest,
            &state,
            (role, repository, record, envelope, context),
            &mut lock,
        )?;
    }
    assert_manifest_lock(&manifest, &lock.ok_or("family publication lock")?)
}

fn assert_manifest_authority(manifest: &Value) {
    assert_eq!(manifest["schema_version"], 2);
    assert_eq!(manifest["repository"], "tailrocks/velnor-new");
    assert_eq!(manifest["source_ref"], "refs/heads/main");
    assert_eq!(manifest["source_commit"], "a".repeat(40));
    assert_eq!(
        manifest["signer_workflow"],
        ".github/workflows/product-release-images.yml"
    );
    assert_eq!(manifest["signer_ref"], "refs/heads/main");
    assert_eq!(
        manifest["workflow_authority_sha"],
        manifest["source_commit"]
    );
    assert_eq!(
        manifest["trusted_signing_identity"]["oidc_issuer"],
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        manifest["trusted_signing_identity"]["certificate_identity"],
        "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main"
    );
    assert_eq!(manifest["platform"], "linux/amd64");
    assert_eq!(manifest["resource_probe"]["protocol_version"], 1);
    assert_eq!(manifest["resource_probe"]["image_user"], "65532:65532");
    assert_eq!(
        manifest["resource_probe"]["entrypoint"],
        "/velnor/resource-probe"
    );
    assert_eq!(manifest["resource_probe"]["working_directory"], "/");
}

fn assert_image_entry(
    fixture: &FamilyFixture,
    manifest: &Value,
    state: &Value,
    entry: (&str, &str, &str, &str, &str),
    family_lock: &mut Option<Value>,
) -> Result<(), Box<dyn Error>> {
    let (role, repository, record_name, envelope_name, context_path) = entry;
    let record: Value = serde_json::from_slice(&fs::read(fixture.assets.join(record_name))?)?;
    let candidate_lock = record.get("lock").ok_or("publication lock")?;
    if let Some(expected) = family_lock {
        assert_eq!(candidate_lock, expected);
    } else {
        *family_lock = Some(candidate_lock.clone());
    }
    let image = &manifest["images"][role];
    assert_eq!(image["repository"], repository);
    assert_eq!(
        image["registry_digest"],
        state["registries"][repository]["tag_digest"]
    );
    assert_eq!(
        image["registry_digest"],
        record["oci"]["platform_manifest_digest"]
    );
    assert_eq!(
        image["platform_manifest_digest"],
        record["oci"]["platform_manifest_digest"]
    );
    assert_eq!(
        image["config_digest"],
        record["oci"]["image_configuration_digest"]
    );
    assert_eq!(
        image["tag"],
        format!(
            "sha-{}",
            manifest["source_commit"].as_str().ok_or("source commit")?
        )
    );
    assert_eq!(
        image["build_source"],
        expected_build_source(fixture, &record, envelope_name, context_path)?
    );
    assert!(!state["registries"][repository]["remote"].is_null());
    Ok(())
}

fn expected_build_source(
    fixture: &FamilyFixture,
    record: &Value,
    envelope_name: &str,
    context_path: &str,
) -> Result<Value, Box<dyn Error>> {
    let envelope: Value = serde_json::from_slice(&fs::read(fixture.assets.join(envelope_name))?)?;
    let statement = base64::engine::general_purpose::STANDARD.decode(
        envelope["statement_b64"]
            .as_str()
            .ok_or("provenance statement")?,
    )?;
    let named_contexts = record["provenance"]["named_contexts"]
        .as_object()
        .ok_or("publication named contexts")?
        .iter()
        .map(|(name, context)| {
            Ok((
                name.clone(),
                context["sha256"]
                    .as_str()
                    .ok_or("context digest")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    Ok(serde_json::json!({
        "metadata_sha256":super::super::support::hex(&Sha256::digest(statement)),
        "build_type":"https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
        "source_repository":"https://github.com/tailrocks/velnor-new",
        "source_commit":record["source"]["commit"],
        "context_path":context_path,
        "dockerfile_path":format!("{context_path}/Dockerfile"),
        "builder_platform":"linux/amd64",
        "named_contexts":named_contexts,
    }))
}

fn assert_manifest_lock(manifest: &Value, lock: &Value) -> Result<(), Box<dyn Error>> {
    assert_eq!(manifest["runner_lock_sha256"], lock["sha256"]);
    for field in [
        "runner_version",
        "docker_version",
        "buildx_version",
        "compose_version",
        "node_version",
    ] {
        let name = field.strip_suffix("_version").ok_or("lock field")?;
        assert_eq!(manifest["versions"][name], lock[field]);
    }
    Ok(())
}
