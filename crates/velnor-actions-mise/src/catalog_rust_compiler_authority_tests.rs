//! Manifest audit cannot substitute for authenticated native compiler installation.

use super::*;

#[test]
fn official_manifest_has_the_exact_root_component_closure() {
    let manifest = OfficialRootRustManifest::audit();
    assert_eq!(manifest.version(), "1.98.1");
    assert_eq!(manifest.target(), "x86_64-unknown-linux-gnu");
    assert_eq!(manifest.release_date(), "2026-09-03");
    assert_eq!(
        manifest
            .components()
            .iter()
            .map(|item| item.component())
            .collect::<Vec<_>>(),
        [
            "rustc",
            "cargo",
            "rust-std",
            "clippy-preview",
            "rustfmt-preview"
        ]
    );
    for component in manifest.components() {
        for hash in [component.xz_sha256(), component.gzip_sha256()] {
            assert_eq!(hash.len(), 64);
            assert!(
                hash.bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
        assert!(
            component
                .xz_url()
                .starts_with("https://static.rust-lang.org/dist/2026-09-03/")
        );
        assert!(
            component
                .gzip_url()
                .starts_with("https://static.rust-lang.org/dist/2026-09-03/")
        );
    }
}

#[test]
fn native_root_installation_authority_remains_absent() {
    assert!(RustCompilerArtifactAuthority::require_root_linux().is_err());
}

#[test]
fn receipt_metadata_cannot_enable_an_unguarded_installer() {
    // Private fixture tests the independent source barrier, never production admission.
    let metadata = RustCompilerArtifactAuthority {
        manifest: OfficialRootRustManifest::audit(),
        installation_receipt_sha256: "fixture-only-receipt",
        installed_tree_sha256: "fixture-only-tree",
        transform_abi: "fixture-only-transform",
    };
    let result = metadata.source_intent_install_source(SourceIntentColdRoot::root_linux());
    assert!(matches!(result, Err(MiseError::Contract { problem })
        if problem.contains("source_intent_pre_first_exec_installer_unqualified")));
}

#[test]
fn every_manifest_and_component_field_binds_the_identity() {
    let manifest = OfficialRootRustManifest::audit();
    let original = manifest.qualification_digest();
    let mutations: [fn(&mut OfficialRootRustManifest); 11] = [
        |value| value.version = "1.98.2",
        |value| value.target = "aarch64-unknown-linux-gnu",
        |value| value.manifest_url = "https://fixture.invalid/manifest",
        |value| value.manifest_sha256 = "changed-manifest",
        |value| value.release_date = "2026-09-04",
        |value| value.rust_source_repository = "changed-rust-repository",
        |value| value.rust_source_commit = "changed-rust-commit",
        |value| value.rust_source_tree = "changed-rust-tree",
        |value| value.cargo_source_repository = "changed-cargo-repository",
        |value| value.cargo_source_commit = "changed-cargo-commit",
        |value| value.cargo_source_tree = "changed-cargo-tree",
    ];
    for mutate in mutations {
        let mut changed = manifest;
        mutate(&mut changed);
        assert_ne!(changed.qualification_digest(), original);
    }
    for position in 0..manifest.components.len() {
        for field in 0..5 {
            let mut components = manifest.components.to_vec();
            match field {
                0 => components[position].component = "changed-component",
                1 => components[position].xz_url = "changed-xz-url",
                2 => components[position].xz_sha256 = "changed-xz-hash",
                3 => components[position].gzip_url = "changed-gzip-url",
                _ => components[position].gzip_sha256 = "changed-gzip-hash",
            }
            let changed = OfficialRootRustManifest {
                components: Box::leak(components.into_boxed_slice()),
                ..manifest
            };
            assert_ne!(changed.qualification_digest(), original);
        }
    }
    let empty = OfficialRootRustManifest {
        components: &[],
        ..manifest
    };
    assert_ne!(empty.qualification_digest(), original);
}
