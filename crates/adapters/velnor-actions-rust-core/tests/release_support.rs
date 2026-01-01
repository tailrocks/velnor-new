//! Shared `cargo metadata` builders for release-case tests.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::support::TempDir;
use velnor_actions_rust_core::{
    PublicationGraph, RegistryState, ReleaseError, ReleaseRequest, ReleaseScope, ReleaseSelection,
    parse_metadata_json, publication_graph, select_release_set,
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

/// Root path helper (canonicalized temp dir).
pub(crate) fn root_of(dir: &TempDir) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(dir.path().canonicalize()?)
}

/// Run selection with release enabled.
pub(crate) fn select_of(
    json: &str,
    root: &Path,
    scope: &ReleaseScope,
    affected: &BTreeSet<String>,
    supported: &[String],
) -> Result<ReleaseSelection, ReleaseError> {
    select_release_set(&ReleaseRequest {
        metadata_json: json,
        repo_root: root,
        manifest_hint: "Cargo.toml",
        scope,
        enabled: true,
        affected,
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
