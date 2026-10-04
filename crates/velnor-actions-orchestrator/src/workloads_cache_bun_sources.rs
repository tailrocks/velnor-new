//! Centrally pinned Bun text-lock candidates; public access is proved separately at runtime.

use std::path::Path;

use serde_json::Value;
use velnor_actions_contract::FileIndex;

use crate::safe_read::{RepoRead, read_repo_file};
use crate::workloads::cache_eligibility::{NativeNpmSource, valid_source_candidate};

/// Closed text lock v1/config v1, default registry, no patches or auth config.
pub(crate) fn source_candidates(index: &FileIndex, root: &str) -> Option<Vec<NativeNpmSource>> {
    crate::source_prep::validate_root(if root == "." { "" } else { root }).ok()?;
    if index.skipped_non_utf8() || !config_absent(index.root(), root) {
        return None;
    }
    let prefix = if root == "." {
        String::new()
    } else {
        format!("{root}/")
    };
    let lock = format!("{prefix}bun.lock");
    if !index.contains(&lock) || !index.contains(&format!("{prefix}package.json")) {
        return None;
    }
    let RepoRead::Text(source) = read_repo_file(index.root(), &lock, 4 * 1024 * 1024).ok()? else {
        return None;
    };
    let RepoRead::Text(_) =
        read_repo_file(index.root(), &format!("{prefix}package.json"), 1024 * 1024).ok()?
    else {
        return None;
    };
    parse_lock(&source)
}

fn config_absent(repository: &Path, root: &str) -> bool {
    let directory = repository.join(root);
    let mut current = Some(directory.as_path());
    while let Some(path) = current {
        if std::fs::symlink_metadata(path)
            .map_or(true, |metadata| metadata.file_type().is_symlink())
        {
            return false;
        }
        for name in [".npmrc", "bunfig.toml", "bun.lockb"] {
            match std::fs::symlink_metadata(path.join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return false,
            }
        }
        if path == repository || path == repository.join(".") {
            return true;
        }
        current = path.parent();
    }
    false
}

fn parse_lock(source: &str) -> Option<Vec<NativeNpmSource>> {
    let lock: Value = serde_json::from_str(&jsonc(source)?).ok()?;
    if lock.get("lockfileVersion")?.as_u64()? != 1
        || lock.get("configVersion")?.as_u64()? != 1
        || lock.get("patchedDependencies").is_some()
        || !lock.get("workspaces")?.is_object()
    {
        return None;
    }
    let packages = lock.get("packages")?.as_object()?;
    if packages.len() > 1024 {
        return None;
    }
    let mut sources = Vec::new();
    for package in packages.values() {
        let package = package.as_array()?;
        if package.len() != 4 || !package[2].is_object() {
            return None;
        }
        let (name, version) = package[0].as_str()?.rsplit_once('@')?;
        let basename = name.rsplit('/').next()?;
        let resolved = format!("https://registry.npmjs.org/{name}/-/{basename}-{version}.tgz");
        let declared = package[1].as_str()?;
        let integrity = package[3].as_str()?;
        if !declared.is_empty() && declared != resolved {
            return None;
        }
        let candidate = NativeNpmSource {
            name: name.to_owned(),
            version: version.to_owned(),
            resolved,
            integrity: integrity.to_owned(),
        };
        if !valid_source_candidate(&candidate) {
            return None;
        }
        sources.push(candidate);
    }
    sources.sort();
    sources.dedup();
    Some(sources)
}

/// Strip comments and trailing commas outside strings, preserving string bytes.
fn jsonc(source: &str) -> Option<String> {
    let mut output = String::new();
    let mut chars = source.chars().peekable();
    let mut quoted = false;
    while let Some(character) = chars.next() {
        if quoted {
            output.push(character);
            if character == '\\' {
                output.push(chars.next()?);
            } else if character == '"' {
                quoted = false;
            }
        } else if character == '"' {
            quoted = true;
            output.push(character);
        } else if character == '/' && chars.peek() == Some(&'/') {
            for character in chars.by_ref() {
                if character == '\n' {
                    output.push('\n');
                    break;
                }
            }
        } else if character == '/' && chars.peek() == Some(&'*') {
            chars.next();
            loop {
                let character = chars.next()?;
                if character == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
            output.push(' ');
        } else if character == ',' {
            let next = chars.clone().find(|character| !character.is_whitespace());
            if !matches!(next, Some('}' | ']')) {
                output.push(character);
            }
        } else {
            output.push(character);
        }
    }
    if quoted { None } else { Some(output) }
}

#[cfg(test)]
#[path = "workloads_cache_bun_sources_tests.rs"]
mod tests;
