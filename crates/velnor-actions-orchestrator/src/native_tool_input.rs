//! Narrow typed projections of the Rust and Mise files used by native build tasks.

use std::collections::BTreeMap;

use crate::native_tool_lock::{NativeMiseLock, parse_native_mise_lock};

const MAX_VALUES: usize = 64;

/// Source identity and the narrowly modeled fields needed to install one
/// declared native build task's selected tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeToolInput {
    pub(crate) sha256: String,
    pub(crate) source: NativeToolSource,
}

/// Supported typed projection of one Mise-owned build input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeToolSource {
    /// Rust version file; fields already validated by the Rust adapter.
    RustToolchain,
    /// Project selectors, task names, settings, and cargo wrapper.
    MiseConfig(NativeMiseConfig),
    /// Exact version/backend/platform artifact records from `mise.lock`.
    MiseLock(NativeMiseLock),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeMiseConfig {
    pub(crate) root_keys: Vec<String>,
    pub(crate) tools: BTreeMap<String, NativeToolSelector>,
    pub(crate) tasks: BTreeMap<String, NativeMiseTask>,
    pub(crate) settings: NativeMiseSettings,
    pub(crate) wrappers: NativeMiseWrappers,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeToolSelector {
    pub(crate) version: Option<String>,
    pub(crate) os: Option<Vec<String>>,
    pub(crate) config_options: BTreeMap<String, String>,
    pub(crate) unsupported_options: Vec<String>,
    pub(crate) valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeMiseTask {
    pub(crate) run_field_present: bool,
    pub(crate) run_commands: Option<Vec<String>>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) task_tools: BTreeMap<String, String>,
    pub(crate) unsupported_fields: Vec<String>,
    pub(crate) valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeMiseSettings {
    pub(crate) present: bool,
    pub(crate) lockfile: Option<bool>,
    pub(crate) idiomatic_version_file_enable_tools: Option<Vec<String>>,
    pub(crate) cargo_binstall: Option<bool>,
    pub(crate) cargo_binstall_only: Option<bool>,
    pub(crate) unsupported_fields: Vec<String>,
    pub(crate) valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NativeMiseWrappers {
    pub(crate) present: bool,
    pub(crate) cargo_command: Option<String>,
    pub(crate) mbx_cargo_shim_mode: Option<String>,
    pub(crate) unsupported_fields: Vec<String>,
    pub(crate) valid_shape: bool,
}

/// Build a typed source projection while retaining the exact raw-file identity.
pub(crate) fn native_mise_source(
    path: &str,
    bytes: &[u8],
    value: &toml::Value,
) -> Option<NativeToolInput> {
    let source = match std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
    {
        Some("mise.toml") => NativeToolSource::MiseConfig(parse_native_mise_config(value)?),
        Some("mise.lock") => NativeToolSource::MiseLock(parse_native_mise_lock(value)?),
        _ => return None,
    };
    Some(NativeToolInput {
        sha256: crate::cover_identity::generator::sha256_hex(bytes),
        source,
    })
}

fn parse_native_mise_config(value: &toml::Value) -> Option<NativeMiseConfig> {
    let root = value.as_table()?;
    let mut config = NativeMiseConfig {
        root_keys: root.keys().cloned().collect(),
        ..NativeMiseConfig::default()
    };
    if let Some(tools) = root.get("tools") {
        for (name, value) in tools.as_table()? {
            config
                .tools
                .insert(name.clone(), parse_native_tool_selector(value));
        }
    }
    if let Some(tasks) = root.get("tasks") {
        for (name, value) in tasks.as_table()? {
            config
                .tasks
                .insert(name.clone(), crate::native_mise_tasks::parse_task(value));
        }
    }
    if let Some(settings) = root.get("settings") {
        config.settings = parse_native_settings(settings);
    }
    if let Some(wrappers) = root.get("wrappers") {
        config.wrappers = parse_native_wrappers(wrappers);
    }
    Some(config)
}

fn parse_native_tool_selector(value: &toml::Value) -> NativeToolSelector {
    if let Some(version) = value.as_str() {
        return NativeToolSelector {
            version: Some(version.to_owned()),
            valid_shape: true,
            ..NativeToolSelector::default()
        };
    }
    let Some(table) = value.as_table() else {
        return NativeToolSelector::default();
    };
    let version = table
        .get("version")
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned);
    let os = match table.get("os") {
        Some(value) => value.as_array().and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        }),
        None => None,
    };
    let config_options = table
        .get("matching_regex")
        .and_then(toml::Value::as_str)
        .map(|value| BTreeMap::from([("matching_regex".to_owned(), value.to_owned())]))
        .unwrap_or_default();
    let mut unsupported_options = table
        .keys()
        .filter(|key| !matches!(key.as_str(), "version" | "os" | "matching_regex"))
        .cloned()
        .collect::<Vec<_>>();
    if table.contains_key("version") && version.is_none() {
        unsupported_options.push("version.shape".to_owned());
    }
    if table.contains_key("os") && os.is_none() {
        unsupported_options.push("os.shape".to_owned());
    }
    if table.contains_key("matching_regex") && !config_options.contains_key("matching_regex") {
        unsupported_options.push("matching_regex.shape".to_owned());
    }
    let valid_shape = version.is_some()
        && (!table.contains_key("os") || os.is_some())
        && (!table.contains_key("matching_regex") || config_options.contains_key("matching_regex"));
    NativeToolSelector {
        version,
        os,
        config_options,
        unsupported_options,
        valid_shape,
    }
}

fn parse_native_settings(value: &toml::Value) -> NativeMiseSettings {
    let Some(table) = value.as_table() else {
        return NativeMiseSettings::default();
    };
    let cargo = table.get("cargo").and_then(toml::Value::as_table);
    let mut unsupported_fields = table
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "lockfile" | "idiomatic_version_file_enable_tools" | "cargo"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let idiomatic_tools = table
        .get("idiomatic_version_file_enable_tools")
        .and_then(toml::Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Option<Vec<_>>>()
        });
    if table.contains_key("lockfile")
        && table
            .get("lockfile")
            .and_then(toml::Value::as_bool)
            .is_none()
    {
        unsupported_fields.push("lockfile.shape".to_owned());
    }
    if table.contains_key("idiomatic_version_file_enable_tools") && idiomatic_tools.is_none() {
        unsupported_fields.push("idiomatic_version_file_enable_tools.shape".to_owned());
    }
    if let Some(cargo) = cargo {
        unsupported_fields.extend(
            cargo
                .keys()
                .filter(|key| !matches!(key.as_str(), "binstall" | "binstall_only"))
                .map(|key| format!("cargo.{key}")),
        );
        for key in ["binstall", "binstall_only"] {
            if cargo.contains_key(key) && cargo.get(key).and_then(toml::Value::as_bool).is_none() {
                unsupported_fields.push(format!("cargo.{key}.shape"));
            }
        }
    } else if table.contains_key("cargo") {
        unsupported_fields.push("cargo.shape".to_owned());
    }
    NativeMiseSettings {
        present: true,
        lockfile: table.get("lockfile").and_then(toml::Value::as_bool),
        idiomatic_version_file_enable_tools: idiomatic_tools,
        cargo_binstall: cargo
            .and_then(|settings| settings.get("binstall"))
            .and_then(toml::Value::as_bool),
        cargo_binstall_only: cargo
            .and_then(|settings| settings.get("binstall_only"))
            .and_then(toml::Value::as_bool),
        unsupported_fields,
        valid_shape: true,
    }
}

fn parse_native_wrappers(value: &toml::Value) -> NativeMiseWrappers {
    let Some(wrappers) = value.as_table() else {
        return NativeMiseWrappers::default();
    };
    let Some(cargo) = wrappers.get("cargo").and_then(toml::Value::as_table) else {
        return NativeMiseWrappers {
            present: true,
            unsupported_fields: wrappers.keys().cloned().collect(),
            valid_shape: false,
            ..NativeMiseWrappers::default()
        };
    };
    let env = cargo.get("env").and_then(toml::Value::as_table);
    let mut unsupported = wrappers
        .keys()
        .filter(|key| key.as_str() != "cargo")
        .cloned()
        .collect::<Vec<_>>();
    unsupported.extend(
        cargo
            .keys()
            .filter(|key| !matches!(key.as_str(), "command" | "env"))
            .map(|key| format!("cargo.{key}")),
    );
    if let Some(env) = env {
        unsupported.extend(
            env.keys()
                .filter(|key| key.as_str() != "MBX_CARGO_SHIM_MODE")
                .map(|key| format!("cargo.env.{key}")),
        );
        if env.contains_key("MBX_CARGO_SHIM_MODE")
            && env
                .get("MBX_CARGO_SHIM_MODE")
                .and_then(toml::Value::as_str)
                .is_none()
        {
            unsupported.push("cargo.env.MBX_CARGO_SHIM_MODE.shape".to_owned());
        }
    } else if cargo.contains_key("env") {
        unsupported.push("cargo.env.shape".to_owned());
    }
    if cargo.contains_key("command") && cargo.get("command").and_then(toml::Value::as_str).is_none()
    {
        unsupported.push("cargo.command.shape".to_owned());
    }
    NativeMiseWrappers {
        present: true,
        cargo_command: cargo
            .get("command")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        mbx_cargo_shim_mode: env
            .and_then(|table| table.get("MBX_CARGO_SHIM_MODE"))
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned),
        unsupported_fields: unsupported,
        valid_shape: true,
    }
}

/// Convert TOML to JSON so one flattener serves both shapes.
pub(crate) fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

/// Flatten scalar leaves into dotted keys, capped and sorted.
pub(crate) fn flatten_json(value: &serde_json::Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    flatten_into(String::new(), value, &mut out);
    out
}

/// Recurse one value, recording scalar leaves only.
fn flatten_into(prefix: String, value: &serde_json::Value, out: &mut BTreeMap<String, String>) {
    if out.len() >= MAX_VALUES {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let scoped = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_into(scoped, child, out);
            }
        }
        serde_json::Value::Array(items) => {
            let scalars: Vec<String> = items.iter().filter_map(json_scalar).collect();
            if !prefix.is_empty() && !scalars.is_empty() {
                out.insert(prefix, scalars.join(","));
            }
        }
        _ => {
            if !prefix.is_empty()
                && let Some(scalar) = json_scalar(value)
            {
                out.insert(prefix, scalar);
            }
        }
    }
}

/// Scalar text of one JSON value, if it is a scalar.
fn json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(num) => Some(num.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            None
        }
    }
}

#[cfg(test)]
#[path = "impl_mise_build_tools.rs"]
mod tests;
