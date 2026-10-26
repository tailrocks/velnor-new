// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEPS_LOCK_PATH: &str = ".velnor/deps.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateSourcesConfig {
    pub owner: String,

    pub app_id_variable: String,

    pub app_private_key_secret: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npm_scopes: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maven_bundles: Vec<PrivateAssetSource>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release_assets: Vec<PrivateAssetSource>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_trees: Vec<PrivateGitTreeSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateAssetSource {
    pub id: String,

    pub repository: String,

    pub tag: String,

    pub asset: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateGitTreeSource {
    pub id: String,

    pub repository: String,

    pub tag: String,
}

#[must_use]
pub fn is_valid_github_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

#[must_use]
pub fn is_valid_github_repo_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && !value.as_bytes().ends_with(b".git")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[must_use]
pub fn split_repository(value: &str) -> Option<(&str, &str)> {
    let (owner, name) = value.split_once('/')?;
    (is_valid_github_owner(owner) && is_valid_github_repo_name(name)).then_some((owner, name))
}

#[must_use]
pub fn is_valid_source_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[must_use]
pub fn is_valid_source_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && !value.starts_with(['-', '/', '.'])
        && !value.ends_with(['/', '.'])
        && !value.contains("..")
        && !value.contains("//")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b'+'))
}

#[must_use]
pub fn is_valid_asset_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && !value.starts_with(['-', '.'])
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
}

#[must_use]
pub fn is_valid_extract_to(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

fn is_valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !value.starts_with("GITHUB_")
}

fn is_valid_npm_scope(value: &str) -> bool {
    value.strip_prefix('@').is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 100
            && name.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
            })
    })
}

impl PrivateSourcesConfig {
    pub fn source_ids(&self) -> impl Iterator<Item = &str> {
        self.maven_bundles
            .iter()
            .map(|source| source.id.as_str())
            .chain(self.release_assets.iter().map(|source| source.id.as_str()))
            .chain(self.git_trees.iter().map(|source| source.id.as_str()))
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let bad = |path: &str, problem: String| ContractError::config(file, path, problem);
        if !is_valid_github_owner(&self.owner) {
            return Err(bad("private_sources.owner", "invalid_owner".to_owned()));
        }
        if !is_valid_env_name(&self.app_id_variable) {
            return Err(bad(
                "private_sources.app_id_variable",
                "invalid_variable_name".to_owned(),
            ));
        }
        if !is_valid_env_name(&self.app_private_key_secret) {
            return Err(bad(
                "private_sources.app_private_key_secret",
                "invalid_secret_name".to_owned(),
            ));
        }
        let mut scopes = BTreeSet::new();
        for scope in &self.npm_scopes {
            if !is_valid_npm_scope(scope) || !scopes.insert(scope.as_str()) {
                return Err(bad(
                    "private_sources.npm_scopes",
                    format!("invalid_or_duplicate_scope:{scope}"),
                ));
            }
        }
        let mut ids = BTreeSet::new();
        self.validate_assets(file, &mut ids)?;
        for (index, source) in self.git_trees.iter().enumerate() {
            let path = format!("private_sources.git_trees[{index}]");
            self.check_common(
                file,
                &path,
                &source.id,
                &source.repository,
                &source.tag,
                &mut ids,
            )?;
        }
        Ok(())
    }

    fn validate_assets<'a>(
        &'a self,
        file: &str,
        ids: &mut BTreeSet<&'a str>,
    ) -> Result<(), ContractError> {
        let bad = |path: &str, problem: String| ContractError::config(file, path, problem);
        for (table, sources) in [
            ("maven_bundles", &self.maven_bundles),
            ("release_assets", &self.release_assets),
        ] {
            for (index, source) in sources.iter().enumerate() {
                let path = format!("private_sources.{table}[{index}]");
                self.check_common(
                    file,
                    &path,
                    &source.id,
                    &source.repository,
                    &source.tag,
                    ids,
                )?;
                if !is_valid_asset_name(&source.asset) {
                    return Err(bad(
                        &format!("{path}.asset"),
                        "invalid_asset_name".to_owned(),
                    ));
                }
                match (table, &source.extract_to) {
                    ("maven_bundles", Some(_)) => {
                        return Err(bad(
                            &format!("{path}.extract_to"),
                            "maven_bundle_has_fixed_location".to_owned(),
                        ));
                    }
                    ("release_assets", None) => {
                        return Err(bad(
                            &format!("{path}.extract_to"),
                            "missing_extract_to".to_owned(),
                        ));
                    }
                    (_, Some(dir)) if !is_valid_extract_to(dir) => {
                        return Err(bad(
                            &format!("{path}.extract_to"),
                            "invalid_extract_to".to_owned(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn check_common<'a>(
        &self,
        file: &str,
        path: &str,
        id: &'a str,
        repository: &str,
        tag: &str,
        ids: &mut BTreeSet<&'a str>,
    ) -> Result<(), ContractError> {
        let bad = |key: &str, problem: String| {
            ContractError::config(file, format!("{path}.{key}"), problem)
        };
        if !is_valid_source_id(id) || !ids.insert(id) {
            return Err(bad("id", "invalid_or_duplicate_id".to_owned()));
        }
        match split_repository(repository) {
            Some((owner, _)) if owner == self.owner => {}
            Some(_) => return Err(bad("repository", "repository_outside_owner".to_owned())),
            None => return Err(bad("repository", "invalid_repository".to_owned())),
        }
        if !is_valid_source_tag(tag) {
            return Err(bad("tag", "invalid_tag".to_owned()));
        }
        Ok(())
    }
}
