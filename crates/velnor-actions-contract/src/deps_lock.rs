use crate::config::{
    PrivateSourcesConfig, is_valid_asset_name, is_valid_extract_to, is_valid_source_id,
    is_valid_source_tag, split_repository,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const DEPS_LOCK_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepsLock {
    pub schema: u32,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maven_bundles: Vec<LockedAsset>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release_assets: Vec<LockedAsset>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_trees: Vec<LockedGitTree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedAsset {
    pub id: String,

    pub repository: String,

    pub tag: String,

    pub commit: String,

    pub asset: String,

    pub sha256: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedGitTree {
    pub id: String,

    pub repository: String,

    pub tag: String,

    pub commit: String,
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl DepsLock {
    #[must_use]
    pub fn repositories(&self) -> BTreeSet<&str> {
        self.maven_bundles
            .iter()
            .chain(&self.release_assets)
            .map(|asset| asset.repository.as_str())
            .chain(self.git_trees.iter().map(|tree| tree.repository.as_str()))
            .collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.maven_bundles.is_empty() && self.release_assets.is_empty() && self.git_trees.is_empty()
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.schema != DEPS_LOCK_SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        let bad = |path: String, problem: &str| ContractError::config(file, path, problem);
        let mut ids = BTreeSet::new();
        for (table, assets) in [
            ("maven_bundles", &self.maven_bundles),
            ("release_assets", &self.release_assets),
        ] {
            for (index, asset) in assets.iter().enumerate() {
                let path = format!("{table}[{index}]");
                if !is_valid_source_id(&asset.id) || !ids.insert(asset.id.as_str()) {
                    return Err(bad(format!("{path}.id"), "invalid_or_duplicate_id"));
                }
                if split_repository(&asset.repository).is_none() {
                    return Err(bad(format!("{path}.repository"), "invalid_repository"));
                }
                if !is_valid_source_tag(&asset.tag) {
                    return Err(bad(format!("{path}.tag"), "invalid_tag"));
                }
                if !is_lower_hex(&asset.commit, 40) {
                    return Err(bad(format!("{path}.commit"), "invalid_commit"));
                }
                if !is_valid_asset_name(&asset.asset) {
                    return Err(bad(format!("{path}.asset"), "invalid_asset_name"));
                }
                if !is_lower_hex(&asset.sha256, 64) {
                    return Err(bad(format!("{path}.sha256"), "invalid_sha256"));
                }
                if asset
                    .extract_to
                    .as_deref()
                    .is_some_and(|dir| !is_valid_extract_to(dir))
                {
                    return Err(bad(format!("{path}.extract_to"), "invalid_extract_to"));
                }
            }
        }
        for (index, tree) in self.git_trees.iter().enumerate() {
            let path = format!("git_trees[{index}]");
            if !is_valid_source_id(&tree.id) || !ids.insert(tree.id.as_str()) {
                return Err(bad(format!("{path}.id"), "invalid_or_duplicate_id"));
            }
            if split_repository(&tree.repository).is_none() {
                return Err(bad(format!("{path}.repository"), "invalid_repository"));
            }
            if !is_valid_source_tag(&tree.tag) {
                return Err(bad(format!("{path}.tag"), "invalid_tag"));
            }
            if !is_lower_hex(&tree.commit, 40) {
                return Err(bad(format!("{path}.commit"), "invalid_commit"));
            }
        }
        Ok(())
    }

    pub fn check_matches(
        &self,
        config: &PrivateSourcesConfig,
        file: &str,
    ) -> Result<(), ContractError> {
        let drift =
            |id: &str| ContractError::config(file, "deps.lock", format!("deps_lock_drift:{id}"));
        let extra =
            |id: &str| ContractError::config(file, "deps.lock", format!("deps_lock_extra:{id}"));
        for (declared_assets, locked_assets) in [
            (&config.maven_bundles, &self.maven_bundles),
            (&config.release_assets, &self.release_assets),
        ] {
            let locked: BTreeMap<&str, &LockedAsset> =
                locked_assets.iter().map(|a| (a.id.as_str(), a)).collect();
            for source in declared_assets {
                let entry = locked
                    .get(source.id.as_str())
                    .ok_or_else(|| drift(&source.id))?;
                if entry.repository != source.repository
                    || entry.tag != source.tag
                    || entry.asset != source.asset
                    || entry.extract_to != source.extract_to
                {
                    return Err(drift(&source.id));
                }
            }
            if let Some(stray) = locked_assets
                .iter()
                .find(|entry| !declared_assets.iter().any(|source| source.id == entry.id))
            {
                return Err(extra(&stray.id));
            }
        }
        let locked_trees: BTreeMap<&str, &LockedGitTree> =
            self.git_trees.iter().map(|t| (t.id.as_str(), t)).collect();
        for source in &config.git_trees {
            let entry = locked_trees
                .get(source.id.as_str())
                .ok_or_else(|| drift(&source.id))?;
            if entry.repository != source.repository || entry.tag != source.tag {
                return Err(drift(&source.id));
            }
        }
        if let Some(stray) = self
            .git_trees
            .iter()
            .find(|entry| !config.git_trees.iter().any(|source| source.id == entry.id))
        {
            return Err(extra(&stray.id));
        }
        Ok(())
    }
}

#[must_use]
pub fn cargo_git_source_repository(source: &str, owner: &str) -> Option<String> {
    let rest = source.strip_prefix("git+https://github.com/")?;
    let path = rest.split(['?', '#']).next()?;
    let (found_owner, name) = path.split_once('/')?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    (found_owner == owner && crate::config::is_valid_github_repo_name(name) && !name.contains('/'))
        .then(|| format!("{found_owner}/{name}"))
}

#[must_use]
pub fn token_repository_allowlist<'a>(
    owner: &str,
    cargo_sources: impl IntoIterator<Item = &'a str>,
    lock: Option<&DepsLock>,
) -> Vec<String> {
    let mut names = BTreeSet::new();
    for source in cargo_sources {
        if let Some(full) = cargo_git_source_repository(source, owner)
            && let Some((_, name)) = split_repository(&full)
        {
            names.insert(name.to_owned());
        }
    }
    if let Some(lock) = lock {
        for repository in lock.repositories() {
            if let Some((found, name)) = split_repository(repository)
                && found == owner
            {
                names.insert(name.to_owned());
            }
        }
    }
    names.into_iter().collect()
}
