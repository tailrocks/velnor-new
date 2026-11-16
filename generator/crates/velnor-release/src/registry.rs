// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{RepoPath, RustMetadata};

use crate::error::ReleaseError;
use crate::release_plz::RootConfig;

const CRATES_IO: &str = "crates-io";

fn names_other_registry(publish: &Value) -> Option<String> {
    match publish {
        Value::Array(items) => items.iter().find_map(|item| match item.as_str() {
            Some(CRATES_IO) => None,
            Some(name) => Some(name.to_owned()),
            None => Some(item.to_string()),
        }),
        Value::Boolean(_) => None,
        other => Some(other.to_string()),
    }
}

fn document(manifests: &BTreeMap<String, String>, path: &str) -> Result<Value, ReleaseError> {
    manifests
        .get(path)
        .and_then(|text| text.parse::<toml::Table>().ok())
        .map(Value::Table)
        .ok_or_else(|| ReleaseError::ManifestUnreadable {
            manifest: path.to_owned(),
        })
}

fn inherited(workspace: &Value) -> Option<Value> {
    workspace
        .get("workspace")?
        .get("package")?
        .get("publish")
        .cloned()
}

pub(crate) fn check_publish_registries(
    config: &RootConfig,
    workspace: &RepoPath,
    members: &RustMetadata,
    manifests: &BTreeMap<String, String>,
) -> Result<(), ReleaseError> {
    if !config.publishes_to_crates_io {
        return Ok(());
    }
    let mut owning = None;
    for member in members
        .members
        .iter()
        .filter(|member| config.publishes(member.name.as_str()))
    {
        let path = member.manifest.as_str();
        let manifest = document(manifests, path)?;
        let Some(package) = manifest.get("package") else {
            continue;
        };
        let publish = match package.get("publish") {
            Some(Value::Table(table)) if table.get("workspace") == Some(&Value::Boolean(true)) => {
                if owning.is_none() {
                    owning = Some(document(manifests, workspace.as_str())?);
                }
                owning.as_ref().and_then(inherited)
            }
            other => other.cloned(),
        };
        if let Some(registry) = publish.as_ref().and_then(names_other_registry) {
            return Err(ReleaseError::PublishNamesOtherRegistry {
                root: config.root.clone(),
                manifest: path.to_owned(),
                registry,
            });
        }
    }
    Ok(())
}
