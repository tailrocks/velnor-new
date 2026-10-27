// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::{collections::BTreeSet, io};

use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
    PackageReleasePlanProduct, PackageReleasePolicy, package_release::PackageReleaseChannel,
    package_release::PackageReleasePayloadFormat,
};

mod preview_source_selection_fixture;
use preview_source_selection_fixture::{
    AUTHORITY_SHA, CI_RUN_ID, PROTECTED_SHA, REPOSITORY, REPOSITORY_ID, SOURCE_SHA, TestResult,
    invoke,
};

const CI_WORKFLOW_ID: u64 = 10;
const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

#[test]
fn public_preview_selection_uses_sha_media_and_preserves_parent_proofs() -> TestResult<()> {
    let large_json = invoke("large-json")?;
    if !large_json.status.success() {
        let stderr = String::from_utf8_lossy(&large_json.stderr);
        assert!(
            stderr.contains("operation=commits")
                && stderr.contains("category=stdout_limit")
                && stderr.contains("cap=1048576"),
            "the unchanged implementation must fail at the real bounded commits response, not at fixture setup: {stderr}; requests={}",
            large_json.requests
        );
    }
    assert!(
        large_json.status.success(),
        "selection with an oversized default JSON commit response must succeed through compact SHA media; status={:?}, stderr={}, requests={}",
        large_json.status.code(),
        String::from_utf8_lossy(&large_json.stderr),
        large_json.requests
    );
    assert_eq!(large_json.stdout, Vec::<u8>::new());
    assert_eq!(
        large_json.output,
        format!(
            "source_sha={SOURCE_SHA}\nworkflow_authority_sha={AUTHORITY_SHA}\nci_run_id={CI_RUN_ID}\nci_attempt=1\ncheckout_lfs=false\n"
        )
    );

    let mut successful_identity_shas = BTreeSet::new();
    assert_identity_media(&large_json.requests, &mut successful_identity_shas, true)?;
    for sha in [SOURCE_SHA, AUTHORITY_SHA, PROTECTED_SHA] {
        assert!(
            successful_identity_shas.contains(sha),
            "commit identity request was not observed for {sha}"
        );
    }

    let wrong_sha = invoke("wrong-sha")?;
    assert!(
        !wrong_sha.status.success(),
        "a different valid commit identity must be rejected; stderr={}, gh_calls={}, requests={}",
        String::from_utf8_lossy(&wrong_sha.stderr),
        wrong_sha.gh_calls,
        wrong_sha.requests
    );
    assert!(
        String::from_utf8_lossy(&wrong_sha.stderr)
            .contains("package_publish_preview_commit_mismatch"),
        "wrong valid SHA should retain its caller-specific mismatch: {}; gh_calls={}, requests={}",
        String::from_utf8_lossy(&wrong_sha.stderr),
        wrong_sha.gh_calls,
        wrong_sha.requests
    );
    assert_eq!(
        wrong_sha.output, "",
        "failed selection must not emit partial outputs"
    );
    assert_identity_media(&wrong_sha.requests, &mut BTreeSet::new(), false)?;
    Ok(())
}
fn assert_identity_media(
    requests: &str,
    observed: &mut BTreeSet<String>,
    require_parent_proofs: bool,
) -> TestResult<()> {
    let mut saw_identity_request = false;
    let mut parent_proof_shas = BTreeSet::new();
    for line in requests.lines() {
        let (accept, route) = line
            .split_once('\t')
            .ok_or_else(|| io::Error::other("malformed GH request media receipt"))?;
        let path = route.split('?').next().unwrap_or(route);
        let segments: Vec<_> = path.split('/').collect();
        if segments.len() >= 5
            && segments[segments.len() - 2] == "commits"
            && segments[segments.len() - 3] != "git"
            && is_lowercase_sha(segments[segments.len() - 1])
        {
            saw_identity_request = true;
            observed.insert(segments[segments.len() - 1].to_owned());
            assert_eq!(
                accept, "application/vnd.github.sha",
                "every commit identity response must use SHA media: {line}"
            );
        }
        if segments.len() >= 6
            && segments[segments.len() - 3] == "git"
            && segments[segments.len() - 2] == "commits"
            && is_lowercase_sha(segments[segments.len() - 1])
        {
            parent_proof_shas.insert(segments[segments.len() - 1].to_owned());
            assert_eq!(
                accept, "application/vnd.github+json",
                "git/commits ancestry proof must remain JSON: {line}"
            );
        }
    }
    assert!(
        saw_identity_request,
        "no commit identity request was recorded"
    );
    if require_parent_proofs {
        assert!(parent_proof_shas.contains(AUTHORITY_SHA));
        assert!(parent_proof_shas.contains(PROTECTED_SHA));
    }
    Ok(())
}

fn is_lowercase_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn seed_policy() -> PackageReleasePolicy {
    PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: REPOSITORY.to_owned(),
        default_branch: "main".to_owned(),
        version: CargoVersionPolicy {
            version_package: "preview-probe".to_owned(),
            bindings: vec![CargoVersionBinding {
                package: "preview-probe".to_owned(),
                manifest_path: "Cargo.toml".to_owned(),
                workspace_manifest_path: "Cargo.toml".to_owned(),
            }],
        },
        products: vec![PackageReleasePlanProduct {
            id: "preview-probe".to_owned(),
            package: "preview-probe".to_owned(),
            bins: vec!["preview-probe".to_owned()],
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    }
}

fn workflow_run_event() -> serde_json::Value {
    let repository = serde_json::json!({
        "id": REPOSITORY_ID,
        "full_name": REPOSITORY,
    });
    serde_json::json!({
        "action": "completed",
        "repository": repository,
        "workflow": {
            "id": CI_WORKFLOW_ID,
            "name": "CI",
            "path": CI_WORKFLOW_PATH,
        },
        "workflow_run": {
            "id": CI_RUN_ID,
            "run_attempt": 1,
            "workflow_id": CI_WORKFLOW_ID,
            "name": "CI",
            "path": CI_WORKFLOW_PATH,
            "head_sha": SOURCE_SHA,
            "head_branch": "main",
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "repository": repository,
            "head_repository": repository,
        },
    })
}
