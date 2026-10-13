//! Release-set selection cases (explicit, publishable, registries, paths).
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::release_support::{doc, graph_of, manifest, pkg, registry, root_of, select_of};
use crate::support::{Outcome, TempDir};
use velnor_actions_rust::{
    DEFAULT_TAG_PATTERN, DepKind, EmitOptions, ReleaseError, ReleaseRequest, ReleaseScope,
    ResolvedScope, emit_bootstrap_config, emit_release_plz_config, select_release_set,
};

/// Path to a `testdata` metadata fixture.
fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name)
}

#[test]
fn disabled_release_short_circuits_without_parsing() -> Outcome {
    let dir = TempDir::create("release-disabled")?;
    let root = root_of(&dir)?;
    let scope = ReleaseScope::PublishableWorkspace;
    let affected = BTreeSet::new();
    let selection = select_release_set(&ReleaseRequest {
        metadata_json: "not json",
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: false,
        affected: &affected,
        supported_registries: &[],
    })?;
    assert!(selection.is_empty());
    assert_eq!(selection.scope, ResolvedScope::Disabled);
    let options = EmitOptions {
        tag_pattern: DEFAULT_TAG_PATTERN,
        groups: &[],
    };
    assert!(matches!(
        emit_release_plz_config(&selection, &options),
        Err(ReleaseError::NothingSelected)
    ));
    Ok(())
}

#[test]
fn single_root_package_selects_with_default_registry() -> Outcome {
    let dir = TempDir::create("release-single")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["solo-id"],
        vec![pkg(
            &root,
            "solo-id",
            "solo",
            "1.4.2",
            "Cargo.toml",
            Value::Null,
            vec![],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["solo".to_owned()]);
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert_eq!(selection.names(), vec!["solo"]);
    assert_eq!(
        selection.packages[0].registries,
        vec!["crates-io".to_owned()]
    );
    assert_eq!(selection.packages[0].manifest, "Cargo.toml");
    let graph = graph_of(&selection, &json, &root, &registry(&[]))?;
    assert_eq!(graph.order, vec!["solo".to_owned()]);
    assert_eq!(graph.edges, [] as [velnor_actions_rust::PackagingEdge; 0]);
    Ok(())
}

#[test]
fn explicit_subset_ignores_ci_affected_state() -> Outcome {
    let dir = TempDir::create("release-subset")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id", "b-id", "c-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "alpha",
                "0.2.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "0.3.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "c-id",
                "gamma",
                "0.4.0",
                "crates/c/Cargo.toml",
                Value::Null,
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["gamma".to_owned(), "alpha".to_owned()]);
    let affected = BTreeSet::from(["b-id".to_owned(), "ghost-id".to_owned()]);
    let selection = select_of(&json, &root, &scope, &affected, &[])?;
    assert_eq!(selection.names(), vec!["alpha", "gamma"]);
    assert_eq!(selection.scope, ResolvedScope::Explicit);
    Ok(())
}

#[test]
fn publishable_workspace_skips_private_helpers() -> Outcome {
    let dir = TempDir::create("release-workspace")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["app-id", "help-id"],
        vec![
            pkg(
                &root,
                "app-id",
                "app",
                "2.0.0",
                "crates/app/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "help-id",
                "helper",
                "2.0.0",
                "crates/helper/Cargo.toml",
                json!([]),
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::PublishableWorkspace;
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &[])?;
    assert_eq!(selection.names(), vec!["app"]);
    let explicit = ReleaseScope::Packages(vec!["helper".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &explicit, &BTreeSet::new(), &[]),
        Err(ReleaseError::PublishForbidden { .. })
    ));
    Ok(())
}

#[test]
fn unsupported_registry_fails_closed_until_supported() -> Outcome {
    let dir = TempDir::create("release-registry")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id"],
        vec![pkg(
            &root,
            "a-id",
            "alpha",
            "0.2.0",
            "Cargo.toml",
            json!(["other-reg"]),
            vec![],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &BTreeSet::new(), &[]),
        Err(ReleaseError::UnsupportedRegistry { .. })
    ));
    let supported = vec!["other-reg".to_owned()];
    let selection = select_of(&json, &root, &scope, &BTreeSet::new(), &supported)?;
    assert_eq!(
        selection.packages[0].registries,
        vec!["other-reg".to_owned()]
    );
    Ok(())
}

#[test]
fn unknown_duplicate_invalid_and_external_selection_fail() -> Outcome {
    let dir = TempDir::create("release-names")?;
    let root = root_of(&dir)?;
    let outside = TempDir::create("release-outside")?;
    let ext_manifest = outside
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();
    let mut external = pkg(
        &root,
        "a-id",
        "alpha",
        "0.2.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    let mut ext_pkg = pkg(
        &root,
        "ext-id",
        "external",
        "1.0.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    ext_pkg["manifest_path"] = Value::String(ext_manifest);
    external["manifest_path"] = Value::String(manifest(&root, "Cargo.toml"));
    let json = doc(&root, vec!["a-id"], vec![external, ext_pkg]);
    let affected = BTreeSet::new();
    for (names, check) in [
        (vec!["ghost"], "unknown"),
        (vec!["alpha", "alpha"], "duplicate"),
        (vec!["bad name!"], "invalid"),
        (vec!["external"], "external"),
    ] {
        let scope = ReleaseScope::Packages(names.iter().map(ToString::to_string).collect());
        let result = select_of(&json, &root, &scope, &affected, &[]);
        let ok = match check {
            "unknown" => matches!(result, Err(ReleaseError::UnknownPackage { .. })),
            "duplicate" => matches!(result, Err(ReleaseError::DuplicateSelection { .. })),
            "invalid" => matches!(result, Err(ReleaseError::InvalidSelectionName { .. })),
            _ => matches!(result, Err(ReleaseError::NotWorkspaceMember { .. })),
        };
        assert!(ok, "selection {names:?} must fail closed as {check}");
    }
    Ok(())
}

#[test]
fn escaping_and_outside_manifests_fail_closed() -> Outcome {
    let dir = TempDir::create("release-paths")?;
    let root = root_of(&dir)?;
    let sneaky = format!("{}/crates/../sneaky/Cargo.toml", root.to_string_lossy());
    let mut escaped = pkg(
        &root,
        "a-id",
        "alpha",
        "0.2.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    escaped["manifest_path"] = Value::String(sneaky);
    let json = doc(&root, vec!["a-id"], vec![escaped]);
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &BTreeSet::new(), &[]),
        Err(ReleaseError::ManifestEscape { .. })
    ));
    let outside = TempDir::create("release-absent")?;
    let abs_manifest = outside
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();
    let mut outside_pkg = pkg(
        &root,
        "b-id",
        "beta",
        "0.2.0",
        "Cargo.toml",
        Value::Null,
        vec![],
    );
    outside_pkg["manifest_path"] = Value::String(abs_manifest);
    let json = doc(&root, vec!["b-id"], vec![outside_pkg]);
    let scope = ReleaseScope::Packages(vec!["beta".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &scope, &BTreeSet::new(), &[]),
        Err(ReleaseError::ManifestOutsideRoot { .. })
    ));
    Ok(())
}

#[test]
fn fixture_workspace_selects_graphs_and_emits() -> Outcome {
    let dir = TempDir::create("release-fixture")?;
    let root = root_of(&dir)?;
    let raw = std::fs::read_to_string(fixture_path("release-workspace.metadata.json"))?;
    let json = raw.replace("@ROOT@", &root.to_string_lossy());
    let affected = BTreeSet::new();
    let scope = ReleaseScope::PublishableWorkspace;
    let selection = select_of(&json, &root, &scope, &affected, &[])?;
    assert_eq!(selection.names(), vec!["app", "codec", "lib"]);
    let helper = ReleaseScope::Packages(vec!["helper".to_owned()]);
    assert!(matches!(
        select_of(&json, &root, &helper, &affected, &[]),
        Err(ReleaseError::PublishForbidden { .. })
    ));
    let state = registry(&[("serde", &["1.0.0"])]);
    let graph = graph_of(&selection, &json, &root, &state)?;
    assert_eq!(graph.order, vec!["codec", "lib", "app"]);
    assert!(graph.edges.iter().all(|edge| edge.kind != DepKind::Dev));
    assert!(graph.edges.iter().any(|edge| edge.kind == DepKind::Build));
    assert!(graph.edges.iter().any(|edge| edge.target.is_some()));
    let options = EmitOptions {
        tag_pattern: DEFAULT_TAG_PATTERN,
        groups: &[],
    };
    let rendered = emit_release_plz_config(&selection, &options)?;
    assert!(rendered.contains("release_always = false"));
    assert!(rendered.contains("name = \"app\""));
    let bootstrap = emit_bootstrap_config(&selection, &options)?;
    assert!(bootstrap.contains("release_always = true"));
    Ok(())
}
