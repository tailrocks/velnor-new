//! Regression boundaries for delivery authority and shared support files.

use super::{
    active_delivery, check_default_branch, check_desktop_policy, check_repository,
    unique_support_files,
};
use crate::OrchestratorError;
use velnor_actions_contract::config::{
    AptDeliveryConfig, DeliveryConfig, DesktopDeliveryConfig, OciReleaseConfig,
    RegistryAuthentication,
};
use velnor_actions_workflow_renderer::RenderedFile;

fn file(path: &str, bytes: &str) -> RenderedFile {
    RenderedFile {
        path: path.to_owned(),
        bytes: bytes.to_owned(),
    }
}

#[test]
fn shared_support_is_sorted_and_identical_duplicates_are_collapsed() {
    let shared = file("scripts/verify.py", "shared admission\n");
    let files = unique_support_files(vec![
        file("workflows/release.yml", "release\n"),
        shared.clone(),
        file("scripts/build.py", "build\n"),
        shared.clone(),
    ])
    .expect("identical helpers may be shared across families");
    assert_eq!(
        files,
        vec![
            file("scripts/build.py", "build\n"),
            shared,
            file("workflows/release.yml", "release\n"),
        ]
    );
}

#[test]
fn shared_support_conflicting_bytes_fail_in_either_order() {
    for bytes in [["first\n", "second\n"], ["second\n", "first\n"]] {
        let error = unique_support_files(vec![
            file("scripts/verify.py", bytes[0]),
            file("scripts/verify.py", bytes[1]),
        ])
        .expect_err("a family cannot replace another family's helper");
        assert!(matches!(
            error,
            OrchestratorError::Contract { problem }
                if problem == "delivery_file_conflict:scripts/verify.py"
        ));
    }
}

#[test]
fn repository_binding_accepts_exact_identity_and_rejects_other_identity() {
    assert!(check_repository("owner/repo", "owner/repo", "delivery.apt").is_ok());
    for configured in ["other/repo", "owner/other", "Owner/repo", ""] {
        let error = check_repository("owner/repo", configured, "delivery.apt")
            .expect_err("configured credentials must match the origin identity");
        assert!(matches!(
            error,
            OrchestratorError::Config { file, key_path, problem }
                if file == ".velnor/config.toml"
                    && key_path == "delivery.apt"
                    && problem == "delivery_repository_mismatch"
        ));
    }
}

#[test]
fn desktop_branch_binding_accepts_literal_default_branches() {
    for branch in ["main", "trunk", "develop", "release/next"] {
        assert!(check_default_branch(branch).is_ok());
    }
    for branch in ["main ", "main' || true", "../main", ""] {
        let error =
            check_default_branch(branch).expect_err("desktop branch cannot escape expressions");
        assert!(matches!(
            error,
            OrchestratorError::Config { file, key_path, problem }
                if file == ".velnor/config.toml"
                    && key_path == "delivery.desktop"
                    && problem == "desktop_default_branch_invalid"
        ));
    }
}

#[test]
fn disabled_desktop_claims_no_repository_or_branch_authority() {
    assert!(check_desktop_policy(false, "owner/repo", "", "develop").is_ok());
    assert!(check_desktop_policy(true, "owner/repo", "owner/repo", "main").is_ok());
    assert!(check_desktop_policy(true, "owner/repo", "", "main").is_err());
    assert!(check_desktop_policy(true, "owner/repo", "owner/repo", "trunk").is_ok());
}

#[test]
fn inactive_delivery_skips_origin_authority() {
    assert!(!active_delivery(&DeliveryConfig::default()));
    let policy = DeliveryConfig {
        desktop: Some(DesktopDeliveryConfig::default()),
        oci: Some(OciReleaseConfig {
            enabled: false,
            registry: "ghcr.io".to_owned(),
            authentication: RegistryAuthentication::GithubToken,
            images: Vec::new(),
        }),
        ..DeliveryConfig::default()
    };
    assert!(!active_delivery(&policy));
}

#[test]
fn each_active_family_requires_origin_authority() {
    let desktop = DeliveryConfig {
        desktop: Some(DesktopDeliveryConfig {
            enabled: true,
            ..DesktopDeliveryConfig::default()
        }),
        ..DeliveryConfig::default()
    };
    assert!(active_delivery(&desktop));
    let oci = DeliveryConfig {
        oci: Some(OciReleaseConfig {
            enabled: true,
            registry: "ghcr.io".to_owned(),
            authentication: RegistryAuthentication::GithubToken,
            images: Vec::new(),
        }),
        ..DeliveryConfig::default()
    };
    assert!(active_delivery(&oci));
    let apt = DeliveryConfig {
        apt: Some(AptDeliveryConfig {
            source_repository: "owner/source".to_owned(),
            consumer_repository: "owner/feed".to_owned(),
            package: "package".to_owned(),
            binary: "binary".to_owned(),
            identity_directory: "identity".to_owned(),
            manifest_schema: "manifest-v1".to_owned(),
            keyring: "keys/public.gpg".to_owned(),
            signer_fingerprint: "A".repeat(40),
            origin: "origin".to_owned(),
            description: "description".to_owned(),
            feed_url: "https://example.com".to_owned(),
            branch: "main".to_owned(),
            schedule: "0 0 * * *".to_owned(),
            signer_workflow: ".github/workflows/release.yml".to_owned(),
            oci_image_repository: "ghcr.io/owner/package".to_owned(),
            oci_signer_workflow: ".github/workflows/release.yml".to_owned(),
        }),
        ..DeliveryConfig::default()
    };
    assert!(active_delivery(&apt));
}
