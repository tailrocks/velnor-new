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

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ResolvedFeatures(BTreeMap<PackageId, BTreeSet<String>>);

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    id: PackageId,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: PackageId,
    features: BTreeSet<String>,
}

pub(crate) fn resolved(bytes: &[u8]) -> Result<ResolvedFeatures, CiError> {
    let metadata: Metadata = serde_json::from_slice(bytes)
        .map_err(|e| CiError::new(format!("invalid resolved cargo metadata: {e}")))?;
    let mut packages = BTreeSet::new();
    for package in metadata.packages {
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
        if !packages.contains(&node.id) {
            return Err(CiError::new(
                "cargo metadata resolve node names an unknown package ID",
            ));
        }
        if features.insert(node.id, node.features).is_some() {
            return Err(CiError::new("duplicate resolve node ID in cargo metadata"));
        }
    }
    Ok(ResolvedFeatures(features))
}
