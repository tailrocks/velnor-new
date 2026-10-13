use super::{codebook, node, resolve_on, rust};
use velnor_actions_contract_config::config::{
    CheckPlatform, QualifiedCargoInstallation, QualifiedTool, QualifiedToolOptions,
};
use velnor_actions_mise_core::checks::{config_for, fingerprint};

#[test]
fn explicit_named_rust_retains_the_qualified_repository_version() {
    let resolved = resolve_on(&[rust()], &["rust".to_owned()], CheckPlatform::LinuxX64)
        .expect("qualified override");
    assert_eq!(resolved.specs, ["rust@1.97.1"]);
    assert_eq!(resolved.declarations[0].version, "1.97.1");
}

#[test]
fn undeclared_tools_never_resolve_from_the_compiled_catalog() {
    for id in ["rust", "cargo-nextest", "node", "unknown"] {
        assert!(resolve_on(&[], &[id.to_owned()], CheckPlatform::LinuxX64).is_err());
    }
    assert!(fingerprint(&[], &["rust@1.98.1".to_owned()]).is_err());
    assert!(fingerprint(&[rust()], &[]).is_err());
}
#[test]
fn installation_dependencies_are_ordered_without_task_graph_edges() {
    let resolved = resolve_on(
        &[codebook(), rust()],
        &["codebook".to_owned()],
        CheckPlatform::LinuxX64,
    )
    .expect("closure");
    let ids: Vec<_> = resolved
        .declarations
        .iter()
        .map(|tool| tool.id.as_str())
        .collect();
    assert_eq!(ids, ["rust", "codebook"]);
    assert!(resolved.specs.contains(&"rust@1.97.1".to_owned()));
    assert!(
        resolved
            .specs
            .contains(&"cargo:codebook-lsp@0.3.42".to_owned())
    );
}
#[test]
fn every_qualification_and_option_dimension_changes_the_identity() {
    let registry = vec![codebook(), rust()];
    let digest = |records: &[QualifiedTool]| {
        resolve_on(records, &["codebook".to_owned()], CheckPlatform::LinuxX64)
            .expect("qualified closure")
            .fingerprint
    };
    let before = digest(&registry);
    let mut changed = registry.clone();
    changed[0].platforms[0].artifacts[0].sha256 = "e".repeat(64);
    assert_ne!(before, digest(&changed));
    let mut changed = registry.clone();
    changed[0].platforms[0].install_tree_sha256 = "e".repeat(64);
    assert_ne!(before, digest(&changed));
    let mut changed = registry.clone();
    changed[0].platforms[0].executables[0].sha256 = "e".repeat(64);
    assert_ne!(before, digest(&changed));
    let mut changed = registry;
    let QualifiedToolOptions::Cargo {
        default_features, ..
    } = &mut changed[0].options
    else {
        panic!("cargo fixture");
    };
    *default_features = true;
    assert_ne!(before, digest(&changed));
}
#[test]
fn qualified_scope_is_platform_specific_and_backend_conflicts_fail() {
    let row = node("node", "24.18.0");
    assert!(
        resolve_on(
            std::slice::from_ref(&row),
            &["node".to_owned()],
            CheckPlatform::MacosArm64,
        )
        .is_err()
    );
    let alternate = node("other-node", "24.17.0");
    assert!(
        resolve_on(
            &[row, alternate],
            &["node".to_owned(), "other-node".to_owned()],
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
}

#[test]
fn pure_projection_preserves_cargo_features_and_explicit_rust_profile() {
    let projected = config_for(&codebook()).expect("typed source options");
    assert!(projected.contains("default-features = false"));
    assert!(!projected.contains("[settings]"));
    let projected = config_for(&rust()).expect("typed Rust options");
    assert!(projected.contains("profile = \"minimal\""));
    assert!(projected.contains("components = [\"clippy\", \"rustfmt\"]"));
    assert!(projected.contains("version = \"1.97.1\""));
}

#[test]
fn prebuilt_cargo_qualification_has_no_synthetic_installer_dependency() {
    let mut tool = codebook();
    tool.depends_on.clear();
    tool.options = QualifiedToolOptions::Cargo {
        default_features: true,
        features: Vec::new(),
        installation: QualifiedCargoInstallation::Prebuilt {
            repository: "codebook/codebook".to_owned(),
        },
    };
    tool.platforms[0].artifacts[0].url =
        "https://github.com/codebook/codebook/releases/download/v0.3.42/codebook-linux-x64.tar.gz"
            .to_owned();
    let resolved = resolve_on(
        std::slice::from_ref(&tool),
        &["codebook".to_owned()],
        CheckPlatform::LinuxX64,
    )
    .expect("direct qualified archive");
    assert_eq!(resolved.declarations, [tool.clone()]);
    assert_eq!(resolved.specs, ["cargo:codebook-lsp@0.3.42"]);
    let projection = config_for(&tool).expect("options only");
    assert!(!projection.contains("[settings]"));
}

#[test]
fn canonical_fingerprint_ignores_dependency_transport_order() {
    let resolved = resolve_on(
        &[codebook(), rust()],
        &["codebook".to_owned()],
        CheckPlatform::LinuxX64,
    )
    .expect("closure");
    let mut reversed = resolved.declarations.clone();
    reversed.reverse();
    assert_eq!(
        resolved.fingerprint,
        fingerprint(&reversed, &resolved.specs).expect("canonical closure")
    );
}
