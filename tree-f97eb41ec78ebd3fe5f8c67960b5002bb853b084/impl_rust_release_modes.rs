//! Release modes: virtual workspaces, independence, order, and fail-closed sets.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::support::{Outcome, TempDir};
use velnor_actions_rust_core::{
    DEFAULT_TAG_PATTERN, EmitOptions, PublicationGraph, RegistryState, ReleaseError,
    ReleaseRequest, ReleaseScope, ReleaseSelection, ResolvedScope, VersionGroup,
    emit_release_plz_config, parse_metadata_json, publication_graph, resolve_version_groups,
    select_release_set,
};

/// Absolute manifest path text for `relative` under `root`.
pub(crate) fn manifest(root: &Path, relative: &str) -> String {
    root.join(relative).to_string_lossy().into_owned()
}

/// Minimal package value; `publish` is raw JSON (null or an array).
#[expect(
    clippy::needless_pass_by_value,
    reason = "owned temporaries keep call sites terse"
)]
pub(crate) fn pkg(
    root: &Path,
    id: &str,
    name: &str,
    version: &str,
    rel: &str,
    publish: Value,
    deps: Vec<Value>,
) -> Value {
    let path = manifest(root, rel);
    json!({
        "name": name, "version": version, "id": id, "manifest_path": path,
        "publish": publish,
        "targets": [{"kind": ["lib"], "name": name, "test": true,
                     "doctest": true, "required_features": []}],
        "features": {},
        "dependencies": deps,
    })
}

/// Dependency knobs (defaults describe a normal registry dependency).
#[derive(Default)]
pub(crate) struct DepOpt {
    pub(crate) kind: Option<String>,
    pub(crate) optional: bool,
    pub(crate) target: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) registry: Option<String>,
    pub(crate) path: Option<String>,
}

/// One dependency declaration value.
pub(crate) fn dep(name: &str, req: &str, opt: DepOpt) -> Value {
    let source = opt.source.or_else(|| {
        if opt.path.is_none() {
            Some("registry+https://example.invalid/index".to_owned())
        } else {
            None
        }
    });
    json!({
        "name": name, "req": req,
        "kind": opt.kind, "optional": opt.optional, "target": opt.target,
        "source": source, "registry": opt.registry, "path": opt.path,
    })
}

/// Minimal `cargo metadata` document text.
#[expect(
    clippy::needless_pass_by_value,
    reason = "owned vecs keep call sites terse"
)]
pub(crate) fn doc(root: &Path, members: Vec<&str>, packages: Vec<Value>) -> String {
    let root_text = root.to_string_lossy().into_owned();
    json!({
        "version": 1, "workspace_root": root_text,
        "workspace_members": members, "packages": packages,
    })
    .to_string()
}

/// Run selection with release enabled.
pub(crate) fn select_of(
    json: &str,
    root: &Path,
    scope: &ReleaseScope,
    supported: &[String],
) -> Result<ReleaseSelection, ReleaseError> {
    select_release_set(&ReleaseRequest {
        metadata_json: json,
        repo_root: root,
        manifest_hint: "Cargo.toml",
        scope,
        enabled: true,
        affected: &BTreeSet::new(),
        supported_registries: supported,
    })
}

/// Observed registry state from name-to-versions pairs.
pub(crate) fn registry(entries: &[(&str, &[&str])]) -> RegistryState {
    let published = entries
        .iter()
        .map(|(name, versions)| {
            (
                (*name).to_owned(),
                versions.iter().map(ToString::to_string).collect(),
            )
        })
        .collect();
    RegistryState { published }
}

/// Build the publication graph with default supported registries.
pub(crate) fn graph_of(
    selection: &ReleaseSelection,
    json: &str,
    root: &Path,
    state: &RegistryState,
) -> Result<PublicationGraph, ReleaseError> {
    let known = std::collections::BTreeSet::new();
    let record =
        parse_metadata_json(json, root, "Cargo.toml", &known).map_err(ReleaseError::Metadata)?;
    publication_graph(selection, &record, state, &[])
}

pub(crate) fn root_of(dir: &TempDir) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(dir.path().canonicalize()?)
}

#[test]
fn virtual_workspace_multi_member_selects_explicit() -> Outcome {
    let dir = TempDir::create("release-virtual")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id", "b-id", "c-id"],
        vec![
            pkg(
                &root,
                "a-id",
                "alpha",
                "0.1.0",
                "crates/a/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "b-id",
                "beta",
                "0.2.0",
                "crates/b/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "c-id",
                "gamma",
                "0.3.0",
                "crates/c/Cargo.toml",
                Value::Null,
                vec![],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["gamma".to_owned(), "alpha".to_owned()]);
    let selection = select_of(&json, &root, &scope, &[])?;
    assert_eq!(selection.scope, ResolvedScope::Explicit);
    assert_eq!(selection.names(), vec!["alpha", "gamma"]);
    assert_eq!(selection.packages[0].manifest, "crates/a/Cargo.toml");
    let graph = graph_of(&selection, &json, &root, &registry(&[]))?;
    assert_eq!(graph.order, vec!["alpha".to_owned(), "gamma".to_owned()]);
    let all = select_of(&json, &root, &ReleaseScope::PublishableWorkspace, &[])?;
    assert_eq!(all.names(), vec!["alpha", "beta", "gamma"]);
    Ok(())
}

#[test]
fn independent_versions_stay_unforced_without_groups() -> Outcome {
    let current = BTreeMap::from([
        ("alpha".to_owned(), "1.0.0".to_owned()),
        ("beta".to_owned(), "2.5.0".to_owned()),
    ]);
    let planned = BTreeMap::from([
        ("alpha".to_owned(), "1.1.0".to_owned()),
        ("beta".to_owned(), "2.5.0".to_owned()),
    ]);
    let changed = BTreeSet::from(["alpha".to_owned()]);
    let resolved = resolve_version_groups(&[], &current, &planned, &changed)?;
    assert!(resolved.is_empty(), "no group coordinates nothing");
    let groups = vec![VersionGroup {
        name: "core".to_owned(),
        members: vec!["alpha".to_owned()],
    }];
    let resolved = resolve_version_groups(&groups, &current, &planned, &changed)?;
    assert_eq!(resolved.len(), 1);
    assert!(
        !resolved.contains_key("beta"),
        "outsiders keep independence"
    );
    let dir = TempDir::create("release-independent")?;
    let root = root_of(&dir)?;
    let json = doc(
        &root,
        vec!["a-id"],
        vec![pkg(
            &root,
            "a-id",
            "alpha",
            "1.0.0",
            "Cargo.toml",
            Value::Null,
            vec![],
        )],
    );
    let scope = ReleaseScope::Packages(vec!["alpha".to_owned()]);
    let selection = select_of(&json, &root, &scope, &[])?;
    let options = EmitOptions {
        tag_pattern: DEFAULT_TAG_PATTERN,
        groups: &[],
    };
    let text = emit_release_plz_config(&selection, &options)?;
    assert!(!text.contains("version_group"), "independent emission");
    Ok(())
}

#[test]
fn mixed_registry_state_orders_new_before_published_deps() -> Outcome {
    let dir = TempDir::create("release-mixed")?;
    let root = root_of(&dir)?;
    let lib_path = manifest(&root, "crates/lib");
    let json = doc(
        &root,
        vec!["l-id", "a-id"],
        vec![
            pkg(
                &root,
                "l-id",
                "lib",
                "1.0.0",
                "crates/lib/Cargo.toml",
                Value::Null,
                vec![],
            ),
            pkg(
                &root,
                "a-id",
                "app",
                "1.0.0",
                "crates/app/Cargo.toml",
                Value::Null,
                vec![
                    dep(
                        "lib",
                        "^1",
                        DepOpt {
                            path: Some(lib_path),
                            ..Default::default()
                        },
                    ),
                    dep("serde", "^1", DepOpt::default()),
                ],
            ),
        ],
    );
    let scope = ReleaseScope::Packages(vec!["app".to_owned(), "lib".to_owned()]);
    let selection = select_of(&json, &root, &scope, &[])?;
    let state = registry(&[("serde", &["1.0.0"])]);
    let graph = graph_of(&selection, &json, &root, &state)?;
    assert_eq!(graph.order, vec!["lib".to_owned(), "app".to_owned()]);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].from, "app");
    assert_eq!(graph.edges[0].to, "lib");
    let solo = ReleaseScope::Packages(vec!["app".to_owned()]);
    let selection = select_of(&json, &root, &solo, &[])?;
    let both = registry(&[("serde", &["1.0.0"]), ("lib", &["1.0.0"])]);
    let graph = graph_of(&selection, &json, &root, &both)?;
    assert_eq!(graph.order, vec!["app".to_owned()]);
    assert!(graph.edges.is_empty(), "published deps add no edges");
    Ok(())
}
