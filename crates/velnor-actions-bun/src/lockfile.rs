use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{BunError, jsonc};

pub const BUN_LOCK: &str = "bun.lock";

const UNSUPPORTED: [&str; 5] = [
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
];

pub fn check_js_lockfiles(names: &[&str]) -> Result<(), BunError> {
    for unsupported in UNSUPPORTED {
        if names.contains(&unsupported) {
            return Err(BunError::new("unsupported_js_lockfile", unsupported));
        }
    }
    if names.contains(&BUN_LOCK) {
        Ok(())
    } else {
        Err(BunError::bare("bun_lock_missing"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockWorkspace {
    pub name: Option<String>,

    pub dependencies: BTreeMap<String, String>,
}

impl LockWorkspace {
    #[must_use]
    pub fn workspace_dependencies(&self) -> BTreeSet<&str> {
        self.dependencies
            .iter()
            .filter(|(_, spec)| spec.starts_with("workspace:"))
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BunLock {
    pub workspaces: BTreeMap<String, LockWorkspace>,

    pub versions: BTreeMap<String, String>,

    pub integrity: BTreeMap<String, String>,
}

const DEPENDENCY_TABLES: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

pub fn parse_lock(text: &str) -> Result<BunLock, BunError> {
    let json = jsonc::to_json(text).map_err(|_| BunError::bare("bun_lock_invalid"))?;
    let root: Value =
        serde_json::from_str(&json).map_err(|_| BunError::bare("bun_lock_invalid"))?;
    let table = root
        .get("workspaces")
        .and_then(Value::as_object)
        .ok_or_else(|| BunError::bare("bun_lock_invalid"))?;
    let mut lock = BunLock::default();
    for (key, entry) in table {
        let mut workspace = LockWorkspace {
            name: entry.get("name").and_then(Value::as_str).map(str::to_owned),
            dependencies: BTreeMap::new(),
        };
        for kind in DEPENDENCY_TABLES {
            for (name, spec) in entry
                .get(kind)
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if let Some(spec) = spec.as_str() {
                    workspace.dependencies.insert(name.clone(), spec.to_owned());
                }
            }
        }
        lock.workspaces.insert(key.clone(), workspace);
    }
    for (name, entry) in root
        .get("packages")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if let Some(version) = entry
            .get(0)
            .and_then(Value::as_str)
            .and_then(|ident| ident.rsplit_once('@'))
            .map(|(_, version)| version)
        {
            lock.versions.insert(name.clone(), version.to_owned());
        }
        if let Some(integrity) = entry
            .get(3)
            .and_then(Value::as_str)
            .filter(|value| value.starts_with("sha512-"))
        {
            lock.integrity.insert(name.clone(), integrity.to_owned());
        }
    }
    Ok(lock)
}
