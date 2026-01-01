//! Supplementary release facts absent from the retained metadata graph.
//!
//! Parses the same authoritative `cargo metadata` document for `publish`
//! state, dependency requirements, sources, and registry restrictions.
//! Package identity still comes from [`WorkspaceRecord`](crate::metadata::WorkspaceRecord);
//! the publication graph cross-checks every resolved path edge against it.

use std::collections::BTreeMap;

use crate::metadata::MetadataError;
use crate::metadata_edges::DepKind;
use crate::release_error::ReleaseError;

/// Declared `publish` state of one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishSetting {
    /// No restriction (the default registry).
    Open,
    /// Restricted to the named registries (sorted, deduped).
    Registries(Vec<String>),
    /// `publish = false`.
    Forbidden,
}

/// Provenance of one dependency declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// Registry dependency (default or `registry+` source).
    Registry,
    /// Git dependency (`git+` source).
    Git,
    /// Local path dependency.
    Path,
    /// Any other source value (rejected at graph time).
    Other(String),
}

/// One dependency declaration with packaging-relevant facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepFact {
    /// Cargo package id of the dependent.
    pub from: String,
    /// Dependency name as resolved to the real package.
    pub name: String,
    /// Version requirement text.
    pub req: String,
    /// Dependency kind.
    pub kind: DepKind,
    /// Whether the edge is optional.
    pub optional: bool,
    /// Target filter when target-specific.
    pub target: Option<String>,
    /// Explicit registry restriction, if any.
    pub registry: Option<String>,
    /// Dependency provenance.
    pub source: DepSource,
    /// Resolved package id for path dependencies.
    pub to: Option<String>,
}

/// Supplementary facts keyed by Cargo package id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFacts {
    /// `publish` state per package id.
    pub publish: BTreeMap<String, PublishSetting>,
    /// Dependency declarations per package id.
    pub deps: BTreeMap<String, Vec<DepFact>>,
}

/// Whether `name` is a safe package name (conservative Cargo subset).
///
/// Non-empty, ASCII alphabetic start, then ASCII alphanumeric, `-`, `_`.
#[must_use]
pub(crate) fn validate_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_')
}

/// Whether `path` is a safe repository-relative manifest path.
#[must_use]
pub(crate) fn validate_manifest_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return false;
    }
    if path.bytes().any(|byte| byte.is_ascii_control()) {
        return false;
    }
    let mut segments = path.split('/');
    let mut last = "";
    for segment in &mut segments {
        if segment.is_empty() || segment == ".." {
            return false;
        }
        last = segment;
    }
    last == "Cargo.toml"
}

/// Whether `name` is a safe registry name.
#[must_use]
pub(crate) fn validate_registry_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars
            .all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_' || char == '.')
}

/// Raw `cargo metadata` document (only release facts retained).
#[derive(Debug, serde::Deserialize)]
struct RawReleaseMetadata {
    #[serde(default)]
    packages: Vec<RawReleasePackage>,
}

/// Raw package entry (only release facts retained).
#[derive(Debug, serde::Deserialize)]
struct RawReleasePackage {
    name: String,
    id: String,
    manifest_path: String,
    #[serde(default)]
    publish: Option<Vec<String>>,
    #[serde(default)]
    dependencies: Vec<RawReleaseDep>,
}

/// Raw dependency entry (only release facts retained).
#[derive(Debug, serde::Deserialize)]
struct RawReleaseDep {
    name: String,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    req: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    target: Option<serde_json::Value>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

/// Parse supplementary release facts from authoritative metadata JSON.
///
/// # Errors
///
/// Returns [`ReleaseError`] on malformed documents, bad registry names,
/// unknown dependency kinds, or unresolvable path values.
pub(crate) fn parse_release_facts(
    json: &str,
    manifest_hint: &str,
) -> Result<ReleaseFacts, ReleaseError> {
    let raw: RawReleaseMetadata =
        serde_json::from_str(json).map_err(|err| ReleaseError::FactsInvalid {
            manifest: manifest_hint.to_owned(),
            detail: err.to_string(),
        })?;
    let dirs = manifest_dirs(&raw.packages);
    let mut publish = BTreeMap::new();
    let mut deps = BTreeMap::new();
    for package in &raw.packages {
        publish.insert(package.id.clone(), publish_setting(package)?);
        let mut facts = Vec::with_capacity(package.dependencies.len());
        for dependency in &package.dependencies {
            facts.push(convert_dep(package, dependency, &dirs, manifest_hint)?);
        }
        facts.sort_by(|left: &DepFact, right: &DepFact| {
            (&left.name, &left.req).cmp(&(&right.name, &right.req))
        });
        deps.insert(package.id.clone(), facts);
    }
    Ok(ReleaseFacts { publish, deps })
}

/// Map one package's `publish` value to its setting.
fn publish_setting(package: &RawReleasePackage) -> Result<PublishSetting, ReleaseError> {
    match &package.publish {
        None => Ok(PublishSetting::Open),
        Some(list) if list.is_empty() => Ok(PublishSetting::Forbidden),
        Some(list) => {
            for registry in list {
                if !validate_registry_name(registry) {
                    return Err(ReleaseError::InvalidRegistry {
                        package: package.name.clone(),
                        registry: registry.clone(),
                    });
                }
            }
            let mut registries = list.clone();
            registries.sort();
            registries.dedup();
            Ok(PublishSetting::Registries(registries))
        }
    }
}

/// Map each package manifest directory to its package id.
fn manifest_dirs(packages: &[RawReleasePackage]) -> BTreeMap<String, String> {
    let mut dirs = BTreeMap::new();
    for package in packages {
        let normalized = package.manifest_path.replace('\\', "/");
        let dir = match normalized.rsplit_once('/') {
            Some((dir, _)) => dir.to_owned(),
            None => normalized,
        };
        dirs.entry(dir).or_insert_with(|| package.id.clone());
    }
    dirs
}

/// Convert one dependency declaration to its packaging facts.
fn convert_dep(
    package: &RawReleasePackage,
    dependency: &RawReleaseDep,
    dirs: &BTreeMap<String, String>,
    hint: &str,
) -> Result<DepFact, ReleaseError> {
    if let Some(registry) = dependency.registry.as_deref()
        && !validate_registry_name(registry)
    {
        return Err(ReleaseError::InvalidRegistry {
            package: package.name.clone(),
            registry: registry.to_owned(),
        });
    }
    let name = dependency
        .package
        .clone()
        .unwrap_or_else(|| dependency.name.clone());
    let source = classify_source(dependency);
    let to = match (&source, dependency.path.as_deref()) {
        (DepSource::Path, Some(path)) => Some(resolve_dep_path(package, &name, path, dirs)?),
        (DepSource::Path, None) => {
            return Err(ReleaseError::UnresolvedLocalDep {
                package: package.name.clone(),
                dep: name,
                path: dependency.source.clone().unwrap_or_default(),
            });
        }
        _ => None,
    };
    Ok(DepFact {
        from: package.id.clone(),
        name,
        req: dependency.req.clone(),
        kind: dep_kind(dependency.kind.as_deref(), hint).map_err(ReleaseError::Metadata)?,
        optional: dependency.optional,
        target: dep_target(dependency.target.as_ref(), hint).map_err(ReleaseError::Metadata)?,
        registry: dependency.registry.clone(),
        source,
        to,
    })
}

/// Classify dependency provenance from path and source values.
fn classify_source(dependency: &RawReleaseDep) -> DepSource {
    if dependency.path.is_some() {
        return DepSource::Path;
    }
    match dependency.source.as_deref() {
        None => DepSource::Registry,
        Some(text) if text.starts_with("registry+") => DepSource::Registry,
        Some(text) if text.starts_with("git+") => DepSource::Git,
        Some(text) if text.starts_with("path+") => DepSource::Path,
        Some(text) => DepSource::Other(text.to_owned()),
    }
}

/// Resolve one path dependency value to its target package id.
fn resolve_dep_path(
    package: &RawReleasePackage,
    dep: &str,
    path: &str,
    dirs: &BTreeMap<String, String>,
) -> Result<String, ReleaseError> {
    let dir = path.replace('\\', "/");
    let dir = dir
        .strip_suffix('/')
        .map_or_else(|| dir.clone(), str::to_owned);
    dirs.get(&dir)
        .cloned()
        .ok_or_else(|| ReleaseError::UnresolvedLocalDep {
            package: package.name.clone(),
            dep: dep.to_owned(),
            path: path.to_owned(),
        })
}

/// Map a raw dependency kind (`null` means normal).
fn dep_kind(raw: Option<&str>, hint: &str) -> Result<DepKind, MetadataError> {
    match raw {
        None => Ok(DepKind::Normal),
        Some("build") => Ok(DepKind::Build),
        Some("dev") => Ok(DepKind::Dev),
        Some(other) => Err(MetadataError::UnknownDepKind {
            manifest: hint.to_owned(),
            kind: other.to_owned(),
        }),
    }
}

/// Map a raw target filter (`null` or string).
fn dep_target(
    raw: Option<&serde_json::Value>,
    hint: &str,
) -> Result<Option<String>, MetadataError> {
    match raw {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(cfg)) => Ok(Some(cfg.clone())),
        Some(_) => Err(MetadataError::InvalidJson {
            manifest: hint.to_owned(),
            detail: "dependency target must be string-or-null".to_owned(),
        }),
    }
}
