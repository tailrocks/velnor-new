//! Exact direct-needs artifact handles, never caller-selected mutable names.
use crate::{RenderError, release_jobs::ReleaseRole};
use std::collections::BTreeMap;

pub(super) fn check(
    role: ReleaseRole,
    id: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if role == ReleaseRole::PackagePreparedAnonymous {
        return check_source(id, env);
    }
    let producers: &[(&str, &str, &str)] = match role {
        ReleaseRole::SourceSnapshotForge
        | ReleaseRole::PackagePreparedAnonymous
        | ReleaseRole::PackageAnonymous
        | ReleaseRole::PreparationAnonymous => &[],
        ReleaseRole::PreflightForge
        | ReleaseRole::RegistryPublishOidc
        | ReleaseRole::RegistryPublishBootstrap => &[(
            "RELEASE_PACKAGE_ARTIFACT",
            "release-package",
            "package-artifact",
        )],
        ReleaseRole::ForgePublish => &[
            (
                "RELEASE_PACKAGE_ARTIFACT",
                "release-package",
                "package-artifact",
            ),
            (
                "RELEASE_REGISTRY_RECEIPT_ARTIFACT",
                "release-registry-publish",
                "registry-receipt-artifact",
            ),
        ],
        ReleaseRole::Reconcile => &[
            (
                "RELEASE_PACKAGE_ARTIFACT",
                "release-package",
                "package-artifact",
            ),
            (
                "RELEASE_PREFLIGHT_ARTIFACT",
                "release-preflight",
                "preflight-artifact",
            ),
            (
                "RELEASE_REGISTRY_RECEIPT_ARTIFACT",
                "release-registry-publish",
                "registry-receipt-artifact",
            ),
            (
                "RELEASE_FORGE_RECEIPT_ARTIFACT",
                "release-forge-publish",
                "forge-receipt-artifact",
            ),
        ],
        ReleaseRole::PreparationForge => &[(
            "RELEASE_PREPARE_ARTIFACT",
            "release-preparation-source",
            "artifact",
        )],
    };
    for (prefix, producer, output) in producers {
        for suffix in ["ID", "DIGEST"] {
            let key = format!("{prefix}_{suffix}");
            let expected = format!(
                "${{{{ needs.{producer}.outputs.{output}-{} }}}}",
                suffix.to_ascii_lowercase()
            );
            if env.get(&key) != Some(&expected) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_artifact_binding:{id}:{key}"
                )));
            }
        }
    }
    Ok(())
}

fn check_source(id: &str, env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for (key, output) in [
        (
            "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID",
            "source-snapshot-artifact-id",
        ),
        (
            "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_DIGEST",
            "source-snapshot-artifact-digest",
        ),
        (
            "RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256",
            "source-snapshot-blob-sha256",
        ),
        ("RELEASE_SOURCE_COMMIT_SHA", "source-commit-sha"),
        ("RELEASE_SOURCE_TREE_SHA", "source-tree-sha"),
    ] {
        let expected = format!("${{{{ needs.release-source-snapshot.outputs.{output} }}}}");
        if env.get(key) != Some(&expected) {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_source_binding:{id}:{key}"
            )));
        }
    }
    Ok(())
}
