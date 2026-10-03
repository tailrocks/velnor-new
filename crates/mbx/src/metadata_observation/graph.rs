use super::{MAX_ITEMS, MAX_STDOUT_BYTES, MAX_TEXT, ObservationError, strict_json};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path};

#[path = "relations.rs"]
mod relations;

#[derive(Debug)]
pub(crate) struct PackageObservation {
    pub(crate) id: String,
    pub(crate) source: Option<String>,
    pub(crate) manifest_path: String,
    pub(crate) target_sources: Vec<String>,
    pub(crate) workspace_member: bool,
}

#[derive(Deserialize)]
struct Metadata {
    version: u32,
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
    workspace_root: String,
    target_directory: String,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    source: Value,
    manifest_path: String,
    targets: Vec<Target>,
    dependencies: Vec<ManifestDependency>,
}
#[derive(Deserialize)]
struct ManifestDependency {
    name: String,
    req: String,
    source: Value,
    kind: Value,
    target: Value,
    rename: Value,
    registry: Value,
    optional: bool,
    uses_default_features: bool,
    features: Vec<String>,
}
#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    crate_types: Vec<String>,
    src_path: String,
}
#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
    root: Value,
}
#[derive(Deserialize)]
struct Node {
    id: String,
    dependencies: Vec<String>,
    deps: Vec<Dependency>,
    features: Vec<String>,
}
#[derive(Deserialize)]
struct Dependency {
    name: String,
    pkg: String,
    dep_kinds: Vec<DependencyKind>,
}
#[derive(Deserialize)]
struct DependencyKind {
    kind: Value,
    target: Value,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Vec<PackageObservation>, ObservationError> {
    if bytes.len() > MAX_STDOUT_BYTES {
        return Err(ObservationError::Bounds);
    }
    let metadata: Metadata = serde_json::from_value(strict_json::parse(bytes)?)
        .map_err(|_| ObservationError::InvalidGraph)?;
    if metadata.version != 1
        || metadata.packages.is_empty()
        || metadata.packages.len() > MAX_ITEMS
        || metadata.workspace_members.is_empty()
        || metadata.workspace_members.len() > MAX_ITEMS
        || !valid_path(&metadata.workspace_root)
        || !valid_path(&metadata.target_directory)
    {
        return Err(ObservationError::InvalidGraph);
    }
    let ids: BTreeSet<_> = metadata
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .collect();
    let members: BTreeSet<_> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    if ids.len() != metadata.packages.len()
        || members.len() != metadata.workspace_members.len()
        || !members.is_subset(&ids)
    {
        return Err(ObservationError::InvalidGraph);
    }
    validate_resolve(&metadata.resolve, &ids, &members)?;
    relations::validate(&metadata)?;
    metadata
        .packages
        .iter()
        .map(|package| package_observation(package, &members))
        .collect()
}

fn package_observation(
    package: &Package,
    members: &BTreeSet<&str>,
) -> Result<PackageObservation, ObservationError> {
    let source = match &package.source {
        Value::Null => None,
        Value::String(source) if text(source) => Some(source.clone()),
        _ => return Err(ObservationError::InvalidGraph),
    };
    if !text(&package.id)
        || !text(&package.name)
        || !text(&package.version)
        || !valid_path(&package.manifest_path)
        || package.targets.is_empty()
        || package.targets.len() > 256
        || package.dependencies.len() > MAX_ITEMS
        || package
            .dependencies
            .iter()
            .any(|dependency| !valid_manifest_dependency(dependency))
        || package.targets.iter().any(|target| {
            !text(&target.name)
                || !texts(&target.kind)
                || target.kind.is_empty()
                || !texts(&target.crate_types)
                || target.crate_types.is_empty()
                || !valid_path(&target.src_path)
        })
    {
        return Err(ObservationError::InvalidGraph);
    }
    Ok(PackageObservation {
        id: package.id.clone(),
        source,
        manifest_path: package.manifest_path.clone(),
        target_sources: package
            .targets
            .iter()
            .map(|target| target.src_path.clone())
            .collect(),
        workspace_member: members.contains(package.id.as_str()),
    })
}

fn validate_resolve(
    resolve: &Resolve,
    ids: &BTreeSet<&str>,
    members: &BTreeSet<&str>,
) -> Result<(), ObservationError> {
    let nodes: BTreeSet<_> = resolve.nodes.iter().map(|node| node.id.as_str()).collect();
    if resolve.nodes.len() > MAX_ITEMS || nodes.len() != resolve.nodes.len() || &nodes != ids {
        return Err(ObservationError::InvalidGraph);
    }
    match &resolve.root {
        Value::Null => {}
        Value::String(root) if members.contains(root.as_str()) => {}
        _ => return Err(ObservationError::InvalidGraph),
    }
    for node in &resolve.nodes {
        validate_node(node, ids)?;
    }
    Ok(())
}

fn validate_node(node: &Node, ids: &BTreeSet<&str>) -> Result<(), ObservationError> {
    let dependencies: BTreeSet<_> = node.dependencies.iter().map(String::as_str).collect();
    let detailed: BTreeSet<_> = node.deps.iter().map(|dep| dep.pkg.as_str()).collect();
    if node.dependencies.len() > MAX_ITEMS
        || node.deps.len() > MAX_ITEMS
        || dependencies.len() != node.dependencies.len()
        || dependencies != detailed
        || !dependencies.is_subset(ids)
        || !texts(&node.features)
    {
        return Err(ObservationError::InvalidGraph);
    }
    let mut edges = BTreeSet::new();
    for dep in &node.deps {
        if !text(&dep.name)
            || !edges.insert((&dep.name, &dep.pkg))
            || dep.dep_kinds.is_empty()
            || dep.dep_kinds.len() > MAX_ITEMS
        {
            return Err(ObservationError::InvalidGraph);
        }
        let mut kinds = BTreeSet::new();
        for kind in &dep.dep_kinds {
            let valid_kind = matches!(&kind.kind, Value::Null)
                || matches!(&kind.kind, Value::String(value) if value == "dev" || value == "build");
            let valid_target = matches!(&kind.target, Value::Null)
                || matches!(&kind.target, Value::String(value) if text(value));
            if !valid_kind
                || !valid_target
                || !kinds.insert((kind.kind.to_string(), kind.target.to_string()))
            {
                return Err(ObservationError::InvalidGraph);
            }
        }
    }
    Ok(())
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT && !value.contains('\0')
}

fn valid_manifest_dependency(dependency: &ManifestDependency) -> bool {
    // Preserve Cargo's declared shape; these fields confer no source qualification.
    let _flags = (dependency.optional, dependency.uses_default_features);
    text(&dependency.name)
        && text(&dependency.req)
        && texts(&dependency.features)
        && nullable_text(&dependency.source)
        && nullable_text(&dependency.target)
        && nullable_text(&dependency.rename)
        && nullable_text(&dependency.registry)
        && (dependency.kind.is_null()
            || matches!(&dependency.kind, Value::String(kind) if kind == "dev" || kind == "build"))
}

fn nullable_text(value: &Value) -> bool {
    value.is_null() || matches!(value, Value::String(value) if text(value))
}
fn texts(values: &[String]) -> bool {
    values.len() <= MAX_ITEMS && values.iter().all(|value| text(value))
}
pub(super) fn valid_path(value: &str) -> bool {
    text(value)
        && Path::new(value).is_absolute()
        && !Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}
