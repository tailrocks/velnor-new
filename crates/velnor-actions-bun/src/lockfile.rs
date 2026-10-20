//! Lockfile policy and the `bun.lock` workspace table.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{BunError, jsonc};

/// The only accepted lockfile: Bun's text lockfile.
pub const BUN_LOCK: &str = "bun.lock";

const UNSUPPORTED: [&str; 5] = [
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
];

/// Reject every JavaScript lockfile other than `bun.lock`.
///
/// `names` are the file names directly inside one root. Hard decision 6: a
/// second package manager is not an adapter Velnor carries.
///
/// # Errors
///
/// Returns `unsupported_js_lockfile:<file>` for the first foreign lockfile
/// (in the order npm, yarn, pnpm, binary Bun), or `bun_lock_missing`.
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

/// One entry of the lock's `workspaces` table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockWorkspace {
    /// Package name, absent for an unnamed root.
    pub name: Option<String>,
    /// Every declared dependency: name to specifier.
    pub dependencies: BTreeMap<String, String>,
}

impl LockWorkspace {
    /// Names depended on through a `workspace:` specifier.
    #[must_use]
    pub fn workspace_dependencies(&self) -> BTreeSet<&str> {
        self.dependencies
            .iter()
            .filter(|(_, spec)| spec.starts_with("workspace:"))
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// The facts of `bun.lock` Velnor reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BunLock {
    /// Workspace key (directory relative to the root, `""` for the root) to entry.
    pub workspaces: BTreeMap<String, LockWorkspace>,
    /// Resolved version of each top-level package name.
    pub versions: BTreeMap<String, String>,
    /// Tarball integrity (`sha512-...`) of each top-level package that records one.
    pub integrity: BTreeMap<String, String>,
}

const DEPENDENCY_TABLES: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// Parse the workspace table and resolved top-level versions of `bun.lock`.
///
/// # Errors
///
/// Returns `bun_lock_invalid` when the text is not JSONC or has no
/// `workspaces` object.
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
