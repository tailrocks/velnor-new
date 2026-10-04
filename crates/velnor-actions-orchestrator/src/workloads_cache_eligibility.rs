//! Declared source candidates for optional native transport, never admission.
//!
//! Node collects npm lock candidates without source configuration overrides.
//! Bun's JSONC/binary formats and Gradle's executable resolution logic need
//! independent reviewed parsers/contracts before their archives are readable.

use std::path::{Component, Path};

use serde_json::Value;
use velnor_actions_contract::FileIndex;

pub(crate) const MAX_NPM_SOURCES: usize = 1024;
pub(crate) const MAX_NPM_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;

/// Immutable lock-derived candidate; only credential-free retrieval proves public access.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeNpmSource {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) resolved: String,
    pub(crate) integrity: String,
}

/// Validate a deserialized candidate tuple; this never proves public access.
pub(crate) fn valid_source_candidate(source: &NativeNpmSource) -> bool {
    let path = format!("node_modules/{}", source.name);
    if package_name(&path).as_deref() != Some(source.name.as_str())
        || !exact_version(&source.version)
        || !sha512_integrity(&source.integrity)
    {
        return false;
    }
    let Some(basename) = source.name.rsplit('/').next() else {
        return false;
    };
    source.resolved
        == format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            source.name, basename, source.version
        )
}

/// Declared npm identities for independent public provenance checks.
/// A registry URL can name a private scoped package. Lock bytes alone prove
/// neither public access nor content provenance and never authorize transport.
pub(crate) fn source_candidates(index: &FileIndex, root: &str) -> Option<Vec<NativeNpmSource>> {
    if index.skipped_non_utf8() || !safe_root(root) {
        return None;
    }
    let directory = index.root().join(root);
    if !source_config_absent(index.root(), &directory) {
        return None;
    }
    let prefix = if root == "." {
        String::new()
    } else {
        format!("{root}/")
    };
    let lock_path = format!("{prefix}package-lock.json");
    let manifest_path = format!("{prefix}package.json");
    if !index.contains(&lock_path) || !index.contains(&manifest_path) {
        return None;
    }
    let lock = read_json(index.root(), &lock_path)?;
    let manifest = read_json(index.root(), &manifest_path)?;
    if !candidate_manifest(&manifest) || !candidate_lock(&lock) {
        return None;
    }
    let packages = lock.get("packages")?.as_object()?;
    let mut sources = Vec::new();
    for (path, package) in packages {
        if path.is_empty() {
            continue;
        }
        let name = package_name(path)?;
        if package
            .get("name")
            .is_some_and(|value| value.as_str() != Some(name.as_str()))
        {
            return None;
        }
        let version = package.get("version")?.as_str()?;
        let resolved = package.get("resolved")?.as_str()?;
        let integrity = package.get("integrity")?.as_str()?;
        let source = NativeNpmSource {
            name,
            version: version.to_owned(),
            resolved: resolved.to_owned(),
            integrity: integrity.to_owned(),
        };
        if !valid_source_candidate(&source) {
            return None;
        }
        sources.push(source);
    }
    sources.sort();
    sources.dedup();
    if sources.len() > MAX_NPM_SOURCES
        || serde_json::to_vec(&sources).ok()?.len() > MAX_NPM_DESCRIPTOR_BYTES
    {
        return None;
    }
    Some(sources)
}

fn package_name(path: &str) -> Option<String> {
    let mut components = path.split('/');
    let mut selected = None;
    while let Some(owner) = components.next() {
        if owner != "node_modules" {
            return None;
        }
        let first = components.next()?;
        let name = if let Some(scope) = first.strip_prefix('@') {
            let package = components.next()?;
            if !name_part(scope) || !name_part(package) {
                return None;
            }
            format!("@{scope}/{package}")
        } else {
            if !name_part(first) {
                return None;
            }
            first.to_owned()
        };
        if name.len() > 214 {
            return None;
        }
        selected = Some(name);
    }
    selected
}

fn name_part(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | ".." | "node_modules")
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn exact_version(version: &str) -> bool {
    let (release, build) = version
        .split_once('+')
        .map_or((version, None), |(release, build)| (release, Some(build)));
    if build.is_some_and(|build| !version_identifiers(build, false)) {
        return false;
    }
    let (core, prerelease) = release
        .split_once('-')
        .map_or((release, None), |(core, pre)| (core, Some(pre)));
    if prerelease.is_some_and(|pre| !version_identifiers(pre, true)) {
        return false;
    }
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

fn version_identifiers(value: &str, reject_numeric_zero: bool) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !(reject_numeric_zero
                && part.len() > 1
                && part.starts_with('0')
                && part.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn sha512_integrity(integrity: &str) -> bool {
    let Some(encoded) = integrity.strip_prefix("sha512-") else {
        return false;
    };
    let bytes = encoded.as_bytes();
    bytes.len() == 88
        && bytes.ends_with(b"==")
        && bytes[..86]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
        && matches!(bytes[85], b'A' | b'Q' | b'g' | b'w')
}

fn safe_root(root: &str) -> bool {
    !root.is_empty()
        && !root.contains('\\')
        && Path::new(root)
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

fn source_config_absent(repository: &Path, directory: &Path) -> bool {
    let mut current = Some(directory);
    while let Some(path) = current {
        for name in [".npmrc", "bunfig.toml", "npm-shrinkwrap.json"] {
            match std::fs::symlink_metadata(path.join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return false,
            }
        }
        if path == repository {
            return true;
        }
        current = path.parent();
    }
    false
}

fn read_json(repository: &Path, relative: &str) -> Option<Value> {
    let path = repository.join(relative).canonicalize().ok()?;
    if !path.starts_with(repository) {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn candidate_manifest(manifest: &Value) -> bool {
    let Some(manifest) = manifest.as_object() else {
        return false;
    };
    if manifest.contains_key("workspaces") || !candidate_scripts(manifest.get("scripts")) {
        return false;
    }
    [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ]
    .iter()
    .all(|key| match manifest.get(*key) {
        None => true,
        Some(dependencies) => dependencies.as_object().is_some_and(|dependencies| {
            dependencies.values().all(|version| {
                version.as_str().is_some_and(|version| {
                    !version.is_empty() && !version.contains([':', '/', '\\', '#', '@'])
                })
            })
        }),
    })
}

fn candidate_scripts(scripts: Option<&Value>) -> bool {
    let Some(scripts) = scripts else { return true };
    scripts.as_object().is_some_and(|scripts| {
        ![
            "preinstall",
            "install",
            "postinstall",
            "preprepare",
            "prepare",
            "postprepare",
            "prepublish",
            "prepublishOnly",
            "dependencies",
        ]
        .iter()
        .any(|hook| scripts.contains_key(*hook))
    })
}

fn candidate_lock(lock: &Value) -> bool {
    if !matches!(
        lock.get("lockfileVersion").and_then(Value::as_u64),
        Some(2 | 3)
    ) {
        return false;
    }
    let Some(packages) = lock.get("packages").and_then(Value::as_object) else {
        return false;
    };
    packages.contains_key("")
        && candidate_legacy_dependencies(lock.get("dependencies"))
        && !packages.is_empty()
        && packages.iter().all(|(name, package)| {
            let Some(package) = package.as_object() else {
                return false;
            };
            if package
                .get("link")
                .is_some_and(|value| value != &Value::Bool(false))
                || package
                    .get("hasInstallScript")
                    .is_some_and(|value| value != &Value::Bool(false))
                || package
                    .get("hasShrinkwrap")
                    .is_some_and(|value| value != &Value::Bool(false))
            {
                return false;
            }
            if name.is_empty() {
                return candidate_manifest(&Value::Object(package.clone()));
            }
            name.starts_with("node_modules/")
                && package
                    .get("resolved")
                    .and_then(Value::as_str)
                    .is_some_and(candidate_tarball)
        })
}

fn candidate_legacy_dependencies(dependencies: Option<&Value>) -> bool {
    let Some(dependencies) = dependencies else {
        return true;
    };
    let Some(dependencies) = dependencies.as_object() else {
        return false;
    };
    dependencies.values().all(|package| {
        package
            .get("resolved")
            .and_then(Value::as_str)
            .is_some_and(candidate_tarball)
            && candidate_legacy_dependencies(package.get("dependencies"))
    })
}

fn candidate_tarball(resolved: &str) -> bool {
    resolved
        .strip_prefix("https://registry.npmjs.org/")
        .is_some_and(|path| {
            !path.is_empty()
                && path.contains("/-/")
                && path.ends_with(".tgz")
                && path.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'@' | b'+')
                })
                && !path.split('/').any(|part| matches!(part, "." | ".."))
        })
}

#[cfg(test)]
#[path = "workloads_cache_eligibility_tests.rs"]
mod tests;
