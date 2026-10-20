//! The `package.json` facts Velnor reads.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::BunError;

/// Facts of one `package.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageJson {
    /// `name`.
    pub name: Option<String>,
    /// `version`.
    pub version: Option<String>,
    /// `private` is `true`.
    pub private: bool,
    /// `scripts` names (command text is never read).
    pub scripts: BTreeSet<String>,
    /// Workspace member patterns (`workspaces` array or `workspaces.packages`).
    pub workspaces: Vec<String>,
    /// `packageManager`.
    pub package_manager: Option<String>,
    /// `publishConfig.registry`.
    pub publish_registry: Option<String>,
    /// `dependencies`, `devDependencies`, `optionalDependencies`: name to specifier.
    pub dependencies: BTreeMap<String, String>,
}

/// Parse the facts of `package.json` text.
///
/// # Errors
///
/// Returns `package_json_invalid` for malformed JSON or a non-object root.
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
