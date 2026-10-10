//! Typed projection of the selected current-platform rows in `mise.lock`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeMiseLock {
    pub(crate) root_keys: Vec<String>,
    pub(crate) lockfile_version: Option<i64>,
    pub(crate) tools: BTreeMap<String, Vec<NativeLockedTool>>,
    pub(crate) valid_shape: bool,
}

impl NativeMiseLock {
    /// Whether the lock uses the supported Mise v3 root schema.
    pub(crate) fn has_supported_root_shape(&self) -> bool {
        self.valid_shape
            && self.lockfile_version == Some(3)
            && self.root_keys.len() == 2
            && self.root_keys.iter().any(|key| key == "lockfile_version")
            && self.root_keys.iter().any(|key| key == "tools")
    }
}

impl NativeMiseLock {
    /// Select exactly one lock row for a pinned request, preserving variant identity.
    pub(crate) fn selected_tool(
        &self,
        key: &str,
        version: &str,
        specifier: &str,
        options: &BTreeMap<String, String>,
    ) -> Option<&NativeLockedTool> {
        let mut matching = self.tools.get(key)?.iter().filter(|tool| {
            tool.version.as_deref() == Some(version)
                && tool
                    .specifiers
                    .as_ref()
                    .is_some_and(|specifiers| specifiers.iter().any(|value| value == specifier))
                && &tool.options == options
        });
        let selected = matching.next()?;
        matching.next().is_none().then_some(selected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeLockedTool {
    pub(crate) version: Option<String>,
    pub(crate) backend: Option<String>,
    pub(crate) specifiers: Option<Vec<String>>,
    pub(crate) options: BTreeMap<String, String>,
    pub(crate) platforms: BTreeMap<String, NativeLockedArtifact>,
    pub(crate) unsupported_fields: Vec<String>,
    pub(crate) valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeLockedArtifact {
    pub(crate) checksum: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) url_api: Option<String>,
    pub(crate) signer: Option<String>,
    pub(crate) provenance: Option<String>,
    pub(crate) repository_ids: Option<NativeRepositoryIds>,
    pub(crate) unsupported_fields: Vec<String>,
    pub(crate) valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeRepositoryIds {
    pub(crate) repository: Option<String>,
    pub(crate) owner: Option<String>,
}

/// Normalize the Rust toolchain's component and target options for the lock projection.
pub(crate) fn rust_toolchain_options(
    values: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    for key in ["components", "targets"] {
        if let Some(value) = values.get(key) {
            let mut items = value.split(',').map(ToOwned::to_owned).collect::<Vec<_>>();
            items.sort();
            items.dedup();
            if items.is_empty() || items.iter().any(String::is_empty) {
                return None;
            }
            options.insert(key.to_owned(), items.join(","));
        }
    }
    Some(options)
}

/// Parse supported exact lock rows while preserving unknown fields for rejection.
pub(crate) fn parse_native_mise_lock(value: &toml::Value) -> Option<NativeMiseLock> {
    let root = value.as_table()?;
    let mut lock = NativeMiseLock {
        root_keys: root.keys().cloned().collect(),
        lockfile_version: root
            .get("lockfile_version")
            .and_then(toml::Value::as_integer),
        valid_shape: true,
        ..NativeMiseLock::default()
    };
    let tools = root.get("tools")?.as_table()?;
    for (name, value) in tools {
        let Some(entries) = value.as_array() else {
            lock.tools
                .insert(name.clone(), vec![NativeLockedTool::default()]);
            continue;
        };
        lock.tools.insert(
            name.clone(),
            entries.iter().map(parse_native_locked_tool).collect(),
        );
    }
    Some(lock)
}

fn parse_native_locked_tool(value: &toml::Value) -> NativeLockedTool {
    let Some(entry) = value.as_table() else {
        return NativeLockedTool::default();
    };
    let mut unsupported_fields = Vec::new();
    let mut options = BTreeMap::new();
    let specifiers = parse_lock_specifiers(entry.get("specifiers"), &mut unsupported_fields);
    if let Some(options_table) = entry.get("options") {
        if let Some(table) = options_table.as_table() {
            for (key, option) in table {
                if let Some(text) = native_scalar_or_list(option) {
                    options.insert(key.clone(), text);
                } else {
                    unsupported_fields.push(format!("options.{key}"));
                }
            }
        } else {
            unsupported_fields.push("options.shape".to_owned());
        }
    }
    let mut platforms = BTreeMap::new();
    for (key, item) in entry {
        if let Some(platform) = key.strip_prefix("platforms.") {
            if platforms
                .insert(platform.to_owned(), parse_native_locked_artifact(item))
                .is_some()
            {
                unsupported_fields.push(format!("platforms.{platform}.duplicate"));
            }
        } else if !matches!(
            key.as_str(),
            "version" | "backend" | "specifiers" | "options"
        ) {
            unsupported_fields.push(key.clone());
        }
    }
    NativeLockedTool {
        version: entry
            .get("version")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        backend: entry
            .get("backend")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        specifiers,
        options,
        platforms,
        unsupported_fields,
        valid_shape: true,
    }
}

fn parse_lock_specifiers(
    value: Option<&toml::Value>,
    unsupported_fields: &mut Vec<String>,
) -> Option<Vec<String>> {
    match value {
        Some(toml::Value::Array(values)) => {
            let strings = values
                .iter()
                .map(toml::Value::as_str)
                .collect::<Option<Vec<_>>>();
            match strings {
                Some(values)
                    if !values.is_empty()
                        && values.iter().all(|value| !value.is_empty())
                        && values
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == values.len() =>
                {
                    Some(values.into_iter().map(ToOwned::to_owned).collect())
                }
                _ => {
                    unsupported_fields.push("specifiers.shape".to_owned());
                    None
                }
            }
        }
        Some(_) => {
            unsupported_fields.push("specifiers.shape".to_owned());
            None
        }
        None => {
            unsupported_fields.push("specifiers.missing".to_owned());
            None
        }
    }
}

fn native_scalar_or_list(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(text) => Some(text.clone()),
        toml::Value::Integer(number) => Some(number.to_string()),
        toml::Value::Boolean(flag) => Some(flag.to_string()),
        toml::Value::Array(items) => items
            .iter()
            .map(toml::Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|items| items.join(",")),
        _ => None,
    }
}

fn parse_native_locked_artifact(value: &toml::Value) -> NativeLockedArtifact {
    let Some(table) = value.as_table() else {
        return NativeLockedArtifact::default();
    };
    let mut unsupported_fields = Vec::new();
    let repository_ids = table.get("repository_ids").and_then(|value| {
        let Some(ids) = value.as_table() else {
            unsupported_fields.push("repository_ids.shape".to_owned());
            return None;
        };
        let repository = ids
            .get("repository")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let owner = ids
            .get("owner")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if repository.is_none() {
            unsupported_fields.push("repository_ids.repository".to_owned());
        }
        if ids
            .keys()
            .any(|key| !matches!(key.as_str(), "repository" | "owner"))
        {
            unsupported_fields.push("repository_ids.unknown_field".to_owned());
        }
        if ids.get("owner").is_some() && owner.is_none() {
            unsupported_fields.push("repository_ids.owner".to_owned());
        }
        Some(NativeRepositoryIds { repository, owner })
    });
    NativeLockedArtifact {
        checksum: table
            .get("checksum")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        url: table
            .get("url")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        url_api: table
            .get("url_api")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        signer: table
            .get("signer")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        provenance: table
            .get("provenance")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        repository_ids,
        unsupported_fields: {
            unsupported_fields.extend(
                table
                    .keys()
                    .filter(|key| {
                        !matches!(
                            key.as_str(),
                            "checksum"
                                | "url"
                                | "url_api"
                                | "signer"
                                | "provenance"
                                | "repository_ids"
                        )
                    })
                    .cloned(),
            );
            unsupported_fields
        },
        valid_shape: true,
    }
}
