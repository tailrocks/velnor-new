use std::collections::BTreeMap;

use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation as Op};
use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;

use super::super::{REPO, SHA, bootstrap};
use super::helper;

const QUALIFIED_PLANNING_GH: &str = "/owned/planning/bin/gh";

pub(super) fn artifact_bindings(role: ReleaseRole) -> BTreeMap<String, String> {
    if role == ReleaseRole::PackagePreparedAnonymous {
        return source_artifact_bindings();
    }
    let producers: &[(&str, &str, &str)] = match role {
        ReleaseRole::SourceSnapshotForge
        | ReleaseRole::PackagePreparedAnonymous
        | ReleaseRole::PackageAnonymous
        | ReleaseRole::PreparationAnonymous => &[],
        ReleaseRole::PreflightForge => &[(
            "RELEASE_PACKAGE_ARTIFACT",
            "release-package",
            "package-artifact",
        )],
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap => &[(
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
    let mut bindings = BTreeMap::new();
    for (prefix, producer, output) in producers {
        for suffix in ["ID", "DIGEST"] {
            let lower = suffix.to_ascii_lowercase();
            bindings.insert(
                format!("{prefix}_{suffix}"),
                format!("${{{{ needs.{producer}.outputs.{output}-{lower} }}}}"),
            );
        }
    }
    bindings
}

fn source_artifact_bindings() -> BTreeMap<String, String> {
    [
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
    ]
    .into_iter()
    .map(|(key, output)| {
        (
            key.to_owned(),
            format!("${{{{ needs.release-source-snapshot.outputs.{output} }}}}"),
        )
    })
    .collect()
}

fn package_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "RELEASE_EXPECTED_PACKAGES".to_owned(),
            "{\"widgets\":\"1.2.3\"}".to_owned(),
        ),
        ("RELEASE_MANIFEST".to_owned(), "Cargo.toml".to_owned()),
        ("RELEASE_PUBLISHABLE_WORKSPACE".to_owned(), "0".to_owned()),
        ("RELEASE_REGISTRY".to_owned(), "crates-io".to_owned()),
        ("RELEASE_REPOSITORY".to_owned(), REPO.to_owned()),
        ("RELEASE_RUST_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ])
}

fn admission_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "ADMISSION_EVENT_POLICY".to_owned(),
            "default-branch".to_owned(),
        ),
        ("ADMISSION_REF_KIND".to_owned(), "branch".to_owned()),
        ("APPROVED_DEFAULT_BRANCH".to_owned(), "main".to_owned()),
        ("APPROVED_REPOSITORY".to_owned(), REPO.to_owned()),
        ("APPROVED_SOURCE_SHA".to_owned(), SHA.to_owned()),
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        (
            "VELNOR_ADMISSION_PLANNING_GH".to_owned(),
            QUALIFIED_PLANNING_GH.to_owned(),
        ),
    ])
}

fn helper_environment(
    role: ReleaseRole,
    operation: Op,
    bootstrap_mode: bool,
) -> BTreeMap<String, String> {
    if operation == Op::ReleaseAdmissionDefaultBranch {
        return admission_environment();
    }
    if matches!(
        operation,
        Op::RustPrepareRootLinux | Op::RustReleasePrepareTools
    ) {
        return BTreeMap::new();
    }
    if operation == Op::MiseToolPrepare {
        return BTreeMap::from([(
            "MISE_DATA_DIR".to_owned(),
            velnor_actions_contract::ToolCacheDomain::Full
                .root()
                .to_owned(),
        )]);
    }
    let mut environment = if matches!(
        operation,
        Op::RustReleaseAnonymousPackage | Op::RustReleasePrepareAnonymous
    ) {
        package_environment()
    } else {
        BTreeMap::from([(
            "RELEASE_RECONCILE_POLICY".to_owned(),
            policy(bootstrap_mode),
        )])
    };
    if operation == Op::RustReleaseSourceSnapshot {
        environment.insert("RELEASE_DEFAULT_BRANCH".to_owned(), "main".to_owned());
        environment.insert(
            "VELNOR_SOURCE_SNAPSHOT_GH".to_owned(),
            QUALIFIED_PLANNING_GH.to_owned(),
        );
    }
    if operation == Op::RustReleasePreparedPackage {
        environment.extend([
            (
                "GITHUB_WORKSPACE".to_owned(),
                "${{ github.workspace }}".to_owned(),
            ),
            ("GITHUB_SHA".to_owned(), "${{ github.sha }}".to_owned()),
            (
                "GITHUB_RUN_ID".to_owned(),
                "${{ github.run_id }}".to_owned(),
            ),
            (
                "GITHUB_RUN_ATTEMPT".to_owned(),
                "${{ github.run_attempt }}".to_owned(),
            ),
            ("GITHUB_REF".to_owned(), "${{ github.ref }}".to_owned()),
            (
                "GITHUB_WORKFLOW_REF".to_owned(),
                "${{ github.workflow_ref }}".to_owned(),
            ),
            (
                "GITHUB_WORKFLOW_SHA".to_owned(),
                "${{ github.workflow_sha }}".to_owned(),
            ),
            ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        ]);
    }
    environment.extend(artifact_bindings(role));
    match operation {
        Op::RustReleaseForgePreflight
        | Op::RustReleaseSourceSnapshot
        | Op::RustRegistryArtifactProof
        | Op::RustReleaseReconcile => {
            environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
        }
        Op::RustRegistryPublish => {
            if bootstrap_mode {
                environment.insert(
                    "CARGO_REGISTRY_TOKEN".to_owned(),
                    "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
                );
            } else {
                environment.extend([
                    (
                        "ACTIONS_ID_TOKEN_REQUEST_URL".to_owned(),
                        "${{ env.ACTIONS_ID_TOKEN_REQUEST_URL }}".to_owned(),
                    ),
                    (
                        "ACTIONS_ID_TOKEN_REQUEST_TOKEN".to_owned(),
                        "${{ env.ACTIONS_ID_TOKEN_REQUEST_TOKEN }}".to_owned(),
                    ),
                ]);
            }
        }
        Op::RustForgePublish | Op::RustReleasePrepareForge => {
            environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
        }
        _ => {}
    }
    environment
}

fn policy(bootstrap_mode: bool) -> String {
    let mut plan = bootstrap();
    if bootstrap_mode {
        plan.version = Some("1.2.3".to_owned());
    }
    super::super::impl_renderer_release_jobs::authority::reconciliation(&plan)
        .serialized()
        .expect("release fixture policy")
}

pub(super) fn helper_record(
    role: ReleaseRole,
    operation: Op,
    bootstrap_mode: bool,
) -> CompiledSourceHelper {
    let scope = match operation {
        Op::RustRegistryPublish if bootstrap_mode => {
            NativeCredentialScope::RustRegistryPublishBootstrap
        }
        Op::RustRegistryPublish => NativeCredentialScope::RustRegistryPublishOidc,
        Op::RustReleaseForgePreflight
        | Op::RustReleaseSourceSnapshot
        | Op::RustRegistryArtifactProof
        | Op::ReleaseAdmissionDefaultBranch
        | Op::RustReleaseReconcile => NativeCredentialScope::GithubReadOnly,
        Op::RustForgePublish | Op::RustReleasePrepareForge => {
            NativeCredentialScope::GithubReleasePublish
        }
        Op::RustReleasePreparedPackage => NativeCredentialScope::Anonymous,
        _ => NativeCredentialScope::Anonymous,
    };
    helper(
        operation,
        helper_environment(role, operation, bootstrap_mode),
        scope,
    )
}
