//! Typed projection of the selected current-platform rows in `mise.lock`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeMiseLock {
    pub(crate) root_keys: Vec<String>,
    pub(crate) lockfile_version: Option<i64>,
    pub(crate) tools: BTreeMap<String, NativeLockedTool>,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeLockedTool {
    pub(crate) version: Option<String>,
    pub(crate) backend: Option<String>,
    pub(crate) options: BTreeMap<String, String>,
    pub(crate) linux_x64: Option<NativeLockedArtifact>,
    pub(crate) macos_arm64: Option<NativeLockedArtifact>,
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
    pub(crate) unsupported_fields: Vec<String>,
    pub(crate) valid_shape: bool,
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
            lock.tools.insert(name.clone(), NativeLockedTool::default());
            continue;
        };
        if entries.len() != 1 {
            lock.tools.insert(name.clone(), NativeLockedTool::default());
            continue;
        }
        let Some(entry) = entries[0].as_table() else {
            lock.tools.insert(name.clone(), NativeLockedTool::default());
            continue;
        };
        let mut unsupported_fields = Vec::new();
        let mut options = BTreeMap::new();
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
                platforms.insert(platform.to_owned(), parse_native_locked_artifact(item));
            } else if !matches!(key.as_str(), "version" | "backend" | "options") {
                unsupported_fields.push(key.clone());
            }
        }
        lock.tools.insert(
            name.clone(),
            NativeLockedTool {
                version: entry
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .map(ToOwned::to_owned),
                backend: entry
                    .get("backend")
                    .and_then(toml::Value::as_str)
                    .map(ToOwned::to_owned),
                options,
                linux_x64: platforms.remove("linux-x64"),
                macos_arm64: platforms.remove("macos-arm64"),
                unsupported_fields,
                valid_shape: true,
            },
        );
    }
    Some(lock)
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
        unsupported_fields: table
            .keys()
            .filter(|key| {
                !matches!(
                    key.as_str(),
                    "checksum" | "url" | "url_api" | "signer" | "provenance"
                )
            })
            .cloned()
            .collect(),
        valid_shape: true,
    }
}
