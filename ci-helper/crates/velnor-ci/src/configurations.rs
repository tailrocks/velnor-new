// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::error::CiError;

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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct PackageId(String);

#[derive(Debug)]
pub(crate) struct ResolvedFeatures {
    features: BTreeMap<PackageId, BTreeSet<String>>,
    context_free: bool,
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
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    id: PackageId,
    targets: Option<Vec<Target>>,
}

#[derive(Deserialize)]
struct Target {
    kind: Option<Vec<String>>,
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
    dep_kinds: Option<Vec<serde_json::Value>>,
}

impl Node {
    fn context_free(&self) -> bool {
        self.deps.as_ref().is_some_and(|dependencies| {
            dependencies.iter().all(|dependency| {
                dependency.dep_kinds.as_ref().is_some_and(|kinds| {
                    !kinds.is_empty()
                        && kinds.iter().all(|kind| {
                            kind.get("target").is_some_and(serde_json::Value::is_null)
                                && kind.get("kind").is_some_and(|kind| {
                                    kind.is_null() || kind.as_str() == Some("dev")
                                })
                        })
                })
            })
        })
    }
}

pub(crate) fn resolved(bytes: &[u8]) -> Result<ResolvedFeatures, CiError> {
    let metadata: Metadata = serde_json::from_slice(bytes)
        .map_err(|e| CiError::new(format!("invalid resolved cargo metadata: {e}")))?;
    let mut packages = BTreeSet::new();
    let mut context_free = true;
    for package in metadata.packages {
        context_free &= package.context_free();
        if package.id.0.is_empty() || !packages.insert(package.id) {
            return Err(CiError::new(
                "empty or duplicate package ID in cargo metadata",
            ));
        }
    }
    if metadata.resolve.nodes.is_empty() {
        return Err(CiError::new("cargo metadata resolve has no nodes"));
    }
    let mut features = BTreeMap::new();
    for node in metadata.resolve.nodes {
        context_free &= node.context_free();
        if !packages.contains(&node.id) {
            return Err(CiError::new(
                "cargo metadata resolve node names an unknown package ID",
            ));
        }
        if features.insert(node.id, node.features).is_some() {
            return Err(CiError::new("duplicate resolve node ID in cargo metadata"));
        }
    }
    Ok(ResolvedFeatures {
        features,
        context_free,
    })
}
