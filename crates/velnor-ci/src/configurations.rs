// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::error::CiError;
use crate::escape::inert;

mod contexts;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) features: Vec<String>,
    #[serde(default)]
    pub(crate) no_default_features: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            name: "default".to_owned(),
            features: Vec::new(),
            no_default_features: false,
        }
    }
}

pub(crate) fn requests(bytes: &[u8]) -> Result<Vec<Configuration>, CiError> {
    let requests: Vec<Configuration> = serde_json::from_slice(bytes)
        .map_err(|e| CiError::new(format!("invalid configurations JSON: {e}")))?;
    if requests.is_empty() {
        return Err(CiError::new("configurations must not be empty"));
    }
    let mut names = BTreeSet::new();
    for request in &requests {
        if request.name.is_empty()
            || !request
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(CiError::new(
                "configuration names must use ASCII letters, digits, '_' or '-'",
            ));
        }
        if !names.insert(&request.name) {
            return Err(CiError::new(format!(
                "duplicate configuration name {}",
                request.name
            )));
        }
        if request
            .features
            .iter()
            .any(|f| f.is_empty() || f.starts_with('-'))
        {
            return Err(CiError::new(format!(
                "invalid feature in configuration {}",
                request.name
            )));
        }
    }
    Ok(requests)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct PackageId(String);

#[derive(Debug)]
pub(crate) struct ResolvedFeatures {
    features: BTreeMap<PackageId, BTreeSet<String>>,
    context_free: bool,
    pub(crate) test_targets: BTreeMap<RequiredTestTarget, Availability>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RequiredTestTarget {
    package_id: PackageId,
    package_name: String,
    name: String,
    kinds: BTreeSet<String>,
    required_features: BTreeSet<String>,
}

impl RequiredTestTarget {
    pub(crate) fn description(&self) -> String {
        format!(
            "test target {} ({}) {} [{}] requires [{}]",
            inert(&self.package_name),
            inert(&self.package_id.0),
            inert(&self.name),
            inert(&self.kinds.iter().cloned().collect::<Vec<_>>().join(", ")),
            inert(
                &self
                    .required_features
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Availability {
    Enabled,
    Unavailable,
    Unknown,
    UnknownDependencyFeatures,
}

impl Availability {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown (feature contexts)",
            Self::UnknownDependencyFeatures => "unknown (dependency feature requirements)",
        }
    }
}

impl ResolvedFeatures {
    pub(crate) fn equivalent(
        &self,
        other: &Self,
        request: &Configuration,
        other_request: &Configuration,
    ) -> bool {
        self.features == other.features
            && ((self.context_free && other.context_free)
                || (request.no_default_features == other_request.no_default_features
                    && normalized_features(request) == normalized_features(other_request)))
    }
}

fn normalized_features(configuration: &Configuration) -> BTreeSet<&str> {
    configuration
        .features
        .iter()
        .flat_map(|features| features.split(|c: char| c == ',' || c.is_whitespace()))
        .filter(|feature| !feature.is_empty())
        .collect()
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: BTreeSet<PackageId>,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    id: PackageId,
    name: Option<String>,
    targets: Option<Vec<Target>>,
}

#[derive(Deserialize)]
struct Target {
    name: Option<String>,
    kind: Option<Vec<String>>,
    #[serde(default, rename = "required-features")]
    required_features: BTreeSet<String>,
}

impl Package {
    fn context_free(&self) -> bool {
        self.targets.as_ref().is_some_and(|targets| {
            !targets.is_empty()
                && targets.iter().all(|target| {
                    target.kind.as_ref().is_some_and(|kinds| {
                        !kinds.is_empty() && !kinds.iter().any(|kind| kind == "proc-macro")
                    })
                })
        })
    }
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: PackageId,
    features: BTreeSet<String>,
    deps: Option<Vec<Dependency>>,
}

#[derive(Deserialize)]
struct Dependency {
    pkg: Option<PackageId>,
    dep_kinds: Option<Vec<serde_json::Value>>,
}

impl Dependency {
    fn context_free(&self) -> bool {
        self.dep_kinds.as_ref().is_some_and(|kinds| {
            !kinds.is_empty()
                && kinds.iter().all(|kind| {
                    kind.get("target").is_some_and(serde_json::Value::is_null)
                        && kind
                            .get("kind")
                            .is_some_and(|kind| kind.is_null() || kind.as_str() == Some("dev"))
                })
        })
    }
}

impl Node {
    fn context_free(&self) -> bool {
        self.deps
            .as_ref()
            .is_some_and(|dependencies| dependencies.iter().all(Dependency::context_free))
    }
}

pub(crate) fn resolved(bytes: &[u8]) -> Result<ResolvedFeatures, CiError> {
    let metadata: Metadata = serde_json::from_slice(bytes)
        .map_err(|e| CiError::new(format!("invalid resolved cargo metadata: {e}")))?;
    if metadata.workspace_members.is_empty() {
        return Err(CiError::new("cargo metadata has no workspace members"));
    }
    let mut packages = BTreeMap::new();
    let mut context_free = true;
    for package in metadata.packages {
        context_free &= package.context_free();
        if package.id.0.is_empty() || packages.insert(package.id.clone(), package).is_some() {
            return Err(CiError::new(
                "empty or duplicate package ID in cargo metadata",
            ));
        }
    }
    if metadata.resolve.nodes.is_empty() {
        return Err(CiError::new("cargo metadata resolve has no nodes"));
    }
    let contexts = contexts::FeatureContexts::new(&packages, &metadata.resolve.nodes);
    let mut features = BTreeMap::new();
    for node in metadata.resolve.nodes {
        context_free &= node.context_free();
        if !packages.contains_key(&node.id) {
            return Err(CiError::new(
                "cargo metadata resolve node names an unknown package ID",
            ));
        }
        if features.insert(node.id, node.features).is_some() {
            return Err(CiError::new("duplicate resolve node ID in cargo metadata"));
        }
    }
    let mut test_targets = BTreeMap::new();
    for member in metadata.workspace_members {
        let package = packages.get(&member).ok_or_else(|| {
            CiError::new("cargo metadata workspace member names an unknown package ID")
        })?;
        let member_features = features
            .get(&member)
            .ok_or_else(|| CiError::new("cargo metadata workspace member has no resolve node"))?;
        for target in required_test_targets(package)? {
            let availability = if target
                .required_features
                .iter()
                .any(|feature| feature.contains('/'))
            {
                Availability::UnknownDependencyFeatures
            } else if contexts.uncertain(&member) {
                Availability::Unknown
            } else if target.required_features.is_subset(member_features) {
                Availability::Enabled
            } else {
                Availability::Unavailable
            };
            test_targets.insert(target, availability);
        }
    }
    Ok(ResolvedFeatures {
        features,
        context_free,
        test_targets,
    })
}

fn required_test_targets(package: &Package) -> Result<Vec<RequiredTestTarget>, CiError> {
    let name = package
        .name
        .as_ref()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CiError::new("cargo metadata workspace package has no name"))?;
    let targets = package
        .targets
        .as_ref()
        .filter(|targets| !targets.is_empty())
        .ok_or_else(|| CiError::new("cargo metadata workspace package has no targets"))?;
    let mut required = Vec::new();
    for target in targets {
        let target_name = target
            .name
            .as_ref()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| CiError::new("cargo metadata workspace target has no name"))?;
        let kinds = target
            .kind
            .as_ref()
            .filter(|kinds| !kinds.is_empty())
            .ok_or_else(|| CiError::new("cargo metadata workspace target has no kind"))?;
        if kinds.iter().any(|kind| kind == "test") && !target.required_features.is_empty() {
            required.push(RequiredTestTarget {
                package_id: package.id.clone(),
                package_name: name.clone(),
                name: target_name.clone(),
                kinds: kinds.iter().cloned().collect(),
                required_features: target.required_features.clone(),
            });
        }
    }
    Ok(required)
}
