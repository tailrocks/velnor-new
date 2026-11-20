// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::error::{DetectError, Reason};
use crate::roots::patch_files;
use crate::toml_file::table;

const BUILTIN_PROVIDERS: &[&str] = &[
    "cargo:token",
    "cargo:wincred",
    "cargo:macos-keychain",
    "cargo:libsecret",
];

fn builtin_providers(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(name) => BUILTIN_PROVIDERS.contains(&name.as_str()),
        toml::Value::Array(names) => names.iter().all(builtin_providers),
        _ => false,
    }
}

pub(crate) fn credential_hook(table: &toml::Table) -> Option<String> {
    if table.contains_key("credential-alias") {
        return Some("credential-alias".to_owned());
    }
    let registry = table.get("registry").and_then(toml::Value::as_table);
    let named = table
        .get("registries")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .map(|(name, entry)| (format!("registries.{name}"), entry.as_table()));
    registry
        .map(|registry| ("registry".to_owned(), Some(registry)))
        .into_iter()
        .chain(named)
        .find_map(|(section, entries)| {
            ["credential-provider", "global-credential-providers"]
                .into_iter()
                .find(|key| {
                    entries
                        .and_then(|entries| entries.get(*key))
                        .is_some_and(|value| !builtin_providers(value))
                })
                .map(|key| format!("{section}.{key}"))
        })
}

pub fn reject_release_credential_hooks(
    view: &RepoView,
    manifest: &RepoPath,
) -> Result<(), DetectError> {
    for (file, bytes, _) in patch_files(view, manifest) {
        if file == manifest {
            continue;
        }
        let table = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        if let Some(key) = credential_hook(&table) {
            return Err(DetectError::invalid(
                file,
                Reason::CargoCredentialProvider(key),
            ));
        }
    }
    Ok(())
}
