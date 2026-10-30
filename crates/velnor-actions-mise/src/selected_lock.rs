// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

const HK_SIGNER: &str = "sigstore-oidc:https://github.com/jdx/hk/.github/workflows/release.yml";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectedMiseLock {
    pub root_keys: Vec<String>,
    pub lockfile_version: Option<i64>,
    pub tools: BTreeMap<String, Vec<SelectedMiseTool>>,
    pub valid_shape: bool,
}

impl SelectedMiseLock {
    #[must_use]
    pub fn has_supported_root_shape(&self) -> bool {
        self.valid_shape
            && self.lockfile_version == Some(3)
            && self.root_keys.len() == 2
            && self.root_keys.iter().any(|key| key == "lockfile_version")
            && self.root_keys.iter().any(|key| key == "tools")
    }
}

impl SelectedMiseLock {
    #[must_use]
    pub fn selected_tool(
        &self,
        key: &str,
        version: &str,
        specifier: &str,
        options: &BTreeMap<String, String>,
    ) -> Option<&SelectedMiseTool> {
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
pub struct SelectedMiseTool {
    pub version: Option<String>,
    pub backend: Option<String>,
    pub specifiers: Option<Vec<String>>,
    pub options: BTreeMap<String, String>,
    pub platforms: BTreeMap<String, SelectedMiseArtifact>,
    pub unsupported_fields: Vec<String>,
    pub valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectedMiseArtifact {
    pub checksum: Option<String>,
    pub url: Option<String>,
    pub url_api: Option<String>,
    pub signer: Option<String>,
    pub provenance: Option<String>,
    pub repository_ids: Option<SelectedRepositoryIds>,
    pub unsupported_fields: Vec<String>,
    pub valid_shape: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectedRepositoryIds {
    pub repository: Option<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedHkLockRow {
    pub version: String,
    pub backend: String,
    pub platform: String,
    pub checksum: String,
    pub url: String,
    pub signer: String,
}

pub fn select_hk_lock_row(
    lock: &SelectedMiseLock,
    version: &str,
    platform: &str,
) -> Result<SelectedHkLockRow, &'static str> {
    crate::catalog::validate_concrete_exact_version("hk", version)
        .map_err(|_| "hk_version_not_exact")?;
    if !lock.has_supported_root_shape() {
        return Err("hk_lock_root_shape");
    }
    let rows = lock.tools.get("hk").ok_or("hk_lock_tool_missing")?;
    let [row] = rows.as_slice() else {
        return Err("hk_lock_tool_ambiguous");
    };
    if !row.valid_shape || !row.unsupported_fields.is_empty() {
        return Err("hk_lock_tool_shape");
    }
    if row.version.as_deref() != Some(version) {
        return Err("hk_lock_version_mismatch");
    }
    if row.backend.as_deref() != Some("packslip:github.com/jdx/hk") {
        return Err("hk_lock_backend_mismatch");
    }
    if row
        .specifiers
        .as_ref()
        .is_none_or(|specifiers| specifiers.len() != 1 || specifiers[0].as_str() != version)
    {
        return Err("hk_lock_specifier_mismatch");
    }
    if !row.options.is_empty() {
        return Err("hk_lock_options_unsupported");
    }
    let asset = match platform {
        "linux-x64" => "hk-x86_64-unknown-linux-gnu.tar.gz",
        "linux-arm64" => "hk-aarch64-unknown-linux-gnu.tar.gz",
        _ => return Err("hk_platform_unsupported"),
    };
    let artifact = row
        .platforms
        .get(platform)
        .ok_or("hk_lock_platform_missing")?;
    if !artifact.valid_shape || !artifact.unsupported_fields.is_empty() {
        return Err("hk_lock_artifact_shape");
    }
    let checksum = artifact
        .checksum
        .as_deref()
        .ok_or("hk_lock_checksum_missing")?;
    if !valid_hk_sha256(checksum) {
        return Err("hk_lock_checksum_invalid");
    }
    let expected_url = format!("https://github.com/jdx/hk/releases/download/v{version}/{asset}");
    if artifact.url.as_deref() != Some(expected_url.as_str())
        || artifact.url_api.is_some()
        || artifact.provenance.is_some()
    {
        return Err("hk_lock_url_mismatch");
    }
    if artifact.signer.as_deref() != Some(HK_SIGNER) {
        return Err("hk_lock_signer_mismatch");
    }
    if let Some(ids) = &artifact.repository_ids
        && (ids.owner.as_deref() != Some("jdx") || ids.repository.as_deref() != Some("hk"))
    {
        return Err("hk_lock_repository_mismatch");
    }
    Ok(SelectedHkLockRow {
        version: version.to_owned(),
        backend: row.backend.clone().ok_or("hk_lock_backend_missing")?,
        platform: platform.to_owned(),
        checksum: checksum.to_owned(),
        url: expected_url,
        signer: HK_SIGNER.to_owned(),
    })
}

fn valid_hk_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub fn parse_selected_mise_lock(value: &toml::Value) -> Option<SelectedMiseLock> {
    let root = value.as_table()?;
    let mut lock = SelectedMiseLock {
        root_keys: root.keys().cloned().collect(),
        lockfile_version: root
            .get("lockfile_version")
            .and_then(toml::Value::as_integer),
        valid_shape: true,
        ..SelectedMiseLock::default()
    };
    let tools = root.get("tools")?.as_table()?;
    for (name, value) in tools {
        let Some(entries) = value.as_array() else {
            lock.tools
                .insert(name.clone(), vec![SelectedMiseTool::default()]);
            continue;
        };
        lock.tools.insert(
            name.clone(),
            entries.iter().map(parse_selected_tool).collect(),
        );
    }
    Some(lock)
}

fn parse_selected_tool(value: &toml::Value) -> SelectedMiseTool {
    let Some(entry) = value.as_table() else {
        return SelectedMiseTool::default();
    };
    let mut unsupported_fields = Vec::new();
    let mut options = BTreeMap::new();
    let specifiers = parse_lock_specifiers(entry.get("specifiers"), &mut unsupported_fields);
    if let Some(options_table) = entry.get("options") {
        if let Some(table) = options_table.as_table() {
            for (key, option) in table {
                if let Some(text) = scalar_or_list(option) {
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
                .insert(platform.to_owned(), parse_selected_artifact(item))
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
    SelectedMiseTool {
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

fn scalar_or_list(value: &toml::Value) -> Option<String> {
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

fn parse_selected_artifact(value: &toml::Value) -> SelectedMiseArtifact {
    let Some(table) = value.as_table() else {
        return SelectedMiseArtifact::default();
    };
    let str_field = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned)
    };
    let mut unsupported_fields = Vec::new();
    let repository_ids = table.get("repository_ids").and_then(|value| {
        let Some(ids) = value.as_table() else {
            unsupported_fields.push("repository_ids.shape".to_owned());
            return None;
        };
        let non_empty = |k: &str| {
            ids.get(k)
                .and_then(toml::Value::as_str)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned)
        };
        let repository = non_empty("repository");
        let owner = non_empty("owner");
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
        Some(SelectedRepositoryIds { repository, owner })
    });
    if table
        .get("provenance_verified")
        .is_some_and(|v| v.as_bool().is_none())
    {
        unsupported_fields.push("provenance_verified".to_owned());
    }
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
                        | "provenance_verified"
                        | "repository_ids"
                )
            })
            .cloned(),
    );
    SelectedMiseArtifact {
        checksum: str_field("checksum"),
        url: str_field("url"),
        url_api: str_field("url_api"),
        signer: str_field("signer"),
        provenance: str_field("provenance"),
        repository_ids,
        unsupported_fields,
        valid_shape: true,
    }
}
