// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;

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

fn inherited(root: &Value, key: &str) -> Option<Value> {
    root.get("workspace")?.get("package")?.get(key).cloned()
}

fn within(root: &str, manifest: &str) -> bool {
    root == "." || manifest.starts_with(&format!("{root}/"))
}

pub(crate) fn check_publish_registries(
    config: &RootConfig,
    manifests: &BTreeMap<String, String>,
) -> Result<(), ReleaseError> {
    if !config.publishes_to_crates_io {
        return Ok(());
    }
    let own = if config.root == "." {
        "Cargo.toml".to_owned()
    } else {
        format!("{}/Cargo.toml", config.root)
    };
    let workspace = manifests
        .get(&own)
        .and_then(|text| text.parse::<toml::Table>().ok())
        .map(Value::Table);
    for (path, text) in manifests
        .iter()
        .filter(|(path, _)| within(&config.root, path))
    {
        let Ok(document) = text.parse::<toml::Table>() else {
            continue;
        };
        let document = Value::Table(document);
        let Some(package) = document.get("package") else {
            continue;
        };
        let listed = package
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| config.packages.iter().any(|known| known.name == name));
        if !listed && config.workspace_tags.is_empty() {
            continue;
        }
        let publish = match package.get("publish") {
            Some(Value::Table(table)) if table.get("workspace") == Some(&Value::Boolean(true)) => {
                workspace
                    .as_ref()
                    .and_then(|root| inherited(root, "publish"))
            }
            other => other.cloned(),
        };
        if let Some(registry) = publish.as_ref().and_then(names_other_registry) {
            return Err(ReleaseError::PublishNamesOtherRegistry {
                root: config.root.clone(),
                manifest: path.clone(),
                registry,
            });
        }
    }
    Ok(())
}
