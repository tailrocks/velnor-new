//! Source-cache confidentiality qualification, separate from task execution.
//!
//! Public crates.io sources are qualified for readable transport. Custom
//! registries, replacement sources, credential-provider configuration and Git
//! dependencies require an independently reviewed reader/provenance policy.
//! They still execute normally; qualification only disables archive transport.

use std::collections::BTreeSet;
use std::path::Path;

use crate::discover::workspace_lock;

const PUBLIC_REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
const PUBLIC_SPARSE_REGISTRY: &str = "sparse+https://index.crates.io/";

/// Immutable admission binds the exact selected lock/config evidence to roots.
#[derive(Debug, Clone)]
pub(crate) struct SourceTransportAdmission {
    roots: Vec<String>,
    locks: Vec<(String, String)>,
    configs: Vec<(String, String)>,
}

impl SourceTransportAdmission {
    /// Unknown, malformed or escaping evidence disables optional transport.
    pub(crate) fn new(root: &Path, roots: &[String]) -> Option<Self> {
        if roots.is_empty() {
            return None;
        }
        let root = root.canonicalize().ok()?;
        let configs = public_config(&root)?;
        let mut roots = roots.to_vec();
        roots.sort();
        roots.dedup();
        let mut locks = Vec::with_capacity(roots.len());
        for workspace in &roots {
            velnor_actions_contract::validate_fetch_root(workspace).ok()?;
            let directory = root.join(workspace).canonicalize().ok()?;
            if !directory.is_dir() || !directory.starts_with(&root) {
                return None;
            }
            let lock_path = root.join(workspace_lock(workspace)).canonicalize().ok()?;
            if !lock_path.starts_with(&root) || !lock_path.is_file() {
                return None;
            }
            let lock = std::fs::read_to_string(lock_path).ok()?;
            locks.push((workspace.clone(), lock));
        }
        Self::from_captured(&roots, &locks, &configs)
    }

    /// Revalidate captured evidence using the same public transport policy.
    pub(crate) fn from_captured(
        roots: &[String],
        locks: &[(String, String)],
        configs: &[(String, String)],
    ) -> Option<Self> {
        if roots.is_empty()
            || roots.len() > 4096
            || locks.len() != roots.len()
            || configs.len() > 2
            || !roots.windows(2).all(|pair| pair[0] < pair[1])
        {
            return None;
        }
        let bytes = roots
            .iter()
            .try_fold(0_usize, |size, root| size.checked_add(root.len()))?;
        let bytes = locks
            .iter()
            .chain(configs)
            .try_fold(bytes, |size, (name, value)| {
                size.checked_add(name.len())?.checked_add(value.len())
            })?;
        if bytes > 64 * 1024 * 1024 {
            return None;
        }
        for (root, (lock_root, lock)) in roots.iter().zip(locks) {
            velnor_actions_contract::validate_fetch_root(root).ok()?;
            if root != lock_root || !public_lock(lock) {
                return None;
            }
        }
        if !configs.windows(2).all(|pair| pair[0].0 < pair[1].0)
            || !configs.iter().all(|(name, config)| {
                matches!(name.as_str(), ".cargo/config" | ".cargo/config.toml")
                    && public_config_bytes(config)
            })
        {
            return None;
        }
        Some(Self {
            roots: roots.to_vec(),
            locks: locks.to_vec(),
            configs: configs.to_vec(),
        })
    }

    /// Validated selected roots, sorted and unique.
    pub(crate) fn roots(&self) -> &[String] {
        &self.roots
    }

    /// Root and captured lock bytes; identity never reopens a changed lock.
    pub(crate) fn locks(&self) -> &[(String, String)] {
        &self.locks
    }

    /// Present checkout configuration paths and captured bytes.
    pub(crate) fn configs(&self) -> &[(String, String)] {
        &self.configs
    }
}

fn public_config(root: &Path) -> Option<Vec<(String, String)>> {
    let mut configs = Vec::new();
    for name in [".cargo/config", ".cargo/config.toml"] {
        match std::fs::symlink_metadata(root.join(name)) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        }
        let path = match root.join(name).canonicalize() {
            Ok(path) if path.starts_with(root) => path,
            _ => return None,
        };
        let config = match std::fs::read_to_string(path) {
            Ok(config) => config,
            Err(_) => return None,
        };
        configs.push((name.to_owned(), config));
    }
    Some(configs)
}

fn public_config_bytes(config: &str) -> bool {
    let Ok(parsed) = toml::from_str::<toml::Table>(config) else {
        return false;
    };
    // Only compiler/display settings have reviewed transport semantics.
    parsed.keys().all(|key| {
        matches!(
            key.as_str(),
            "build" | "target" | "term" | "profile" | "alias"
        )
    })
}

fn public_lock(lock: &str) -> bool {
    let Ok(parsed) = toml::from_str::<toml::Table>(lock) else {
        return false;
    };
    if parsed.get("version").and_then(toml::Value::as_integer) != Some(4) {
        return false;
    }
    let Some(packages) = parsed.get("package").and_then(toml::Value::as_array) else {
        return false;
    };
    let mut identities = BTreeSet::new();
    !packages.is_empty()
        && packages.iter().all(|package| {
            public_package(package).is_some_and(|identity| identities.insert(identity))
        })
}

fn public_package(package: &toml::Value) -> Option<(String, String)> {
    let package = package.as_table()?;
    let name = package.get("name")?.as_str()?;
    let version = package.get("version")?.as_str()?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || velnor_actions_rust::release_semver::parse_version(version).is_none()
    {
        return None;
    }
    match package.get("source") {
        None if !package.contains_key("checksum") => {}
        Some(source)
            if matches!(
                source.as_str(),
                Some(PUBLIC_REGISTRY | PUBLIC_SPARSE_REGISTRY)
            ) =>
        {
            let checksum = package.get("checksum")?.as_str()?;
            if checksum.len() != 64
                || !checksum
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return None;
            }
        }
        _ => return None,
    }
    Some((name.to_owned(), version.to_owned()))
}

#[cfg(test)]
#[path = "source_transport_tests.rs"]
mod tests;
