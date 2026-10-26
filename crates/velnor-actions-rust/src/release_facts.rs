// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::metadata::MetadataError;
use crate::metadata_edges::DepKind;
use crate::release_error::ReleaseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishSetting {
    Open,
    Registries(Vec<String>),
    Forbidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    Registry,
    Git,
    Path,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepFact {
    pub from: String,
    pub name: String,
    pub req: String,
    pub kind: DepKind,
    pub optional: bool,
    pub target: Option<String>,
    pub registry: Option<String>,
    pub source: DepSource,
    pub to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFacts {
    pub publish: BTreeMap<String, PublishSetting>,
    pub deps: BTreeMap<String, Vec<DepFact>>,
}

#[must_use]
pub(crate) fn validate_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_')
}

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

#[derive(Debug, serde::Deserialize)]
struct RawReleaseMetadata {
    #[serde(default)]
    packages: Vec<RawReleasePackage>,
}

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
