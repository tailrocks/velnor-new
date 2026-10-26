// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::BunError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageJson {
    pub name: Option<String>,

    pub version: Option<String>,

    pub private: bool,

    pub scripts: BTreeSet<String>,

    pub commands: BTreeMap<String, String>,

    pub workspaces: Vec<String>,

    pub package_manager: Option<String>,

    pub publish_registry: Option<String>,

    pub dependencies: BTreeMap<String, String>,
}

pub fn parse_package_json(text: &str) -> Result<PackageJson, BunError> {
    let root: Value =
        serde_json::from_str(text).map_err(|_| BunError::bare("package_json_invalid"))?;
    let object = root
        .as_object()
        .ok_or_else(|| BunError::bare("package_json_invalid"))?;
    let string = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    let patterns = match object.get("workspaces") {
        Some(Value::Array(items)) => items.as_slice(),
        Some(Value::Object(map)) => map
            .get("packages")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice),
        _ => &[],
    };
    let mut dependencies = BTreeMap::new();
    for kind in ["dependencies", "devDependencies", "optionalDependencies"] {
        for (name, spec) in object
            .get(kind)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(spec) = spec.as_str() {
                dependencies.insert(name.clone(), spec.to_owned());
            }
        }
    }
    Ok(PackageJson {
        name: string("name"),
        version: string("version"),
        private: object.get("private").and_then(Value::as_bool) == Some(true),
        scripts: object
            .get("scripts")
            .and_then(Value::as_object)
            .map(|scripts| scripts.keys().cloned().collect())
            .unwrap_or_default(),
        commands: object
            .get("scripts")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(name, text)| text.as_str().map(|text| (name.clone(), text.to_owned())))
            .collect(),
        workspaces: patterns
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        package_manager: string("packageManager"),
        publish_registry: object
            .get("publishConfig")
            .and_then(|config| config.get("registry"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        dependencies,
    })
}
