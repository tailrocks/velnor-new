// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::CiError;

pub(super) fn resolve(link: &Path, checkout: &Path) -> Result<PathBuf, CiError> {
    let resolved = fs::canonicalize(link).map_err(|e| {
        CiError::new(format!(
            "cache target {} is a symlink that cannot be resolved ({e}); remove it or let the build recreate it",
            link.display()
        ))
    })?;
    let metadata = fs::metadata(&resolved)
        .map_err(|e| CiError::new(format!("cannot inspect {}: {e}", resolved.display())))?;
    if !metadata.is_dir() {
        return Err(CiError::new(format!(
            "cache target {} is a symlink to {}, which is not a directory",
            link.display(),
            resolved.display()
        )));
    }
    let owner = nix::unistd::geteuid().as_raw();
    if metadata.uid() != owner {
        return Err(CiError::new(format!(
            "cache target {} is a symlink to {}, which is owned by another user",
            link.display(),
            resolved.display()
        )));
    }
    let roots = allowed_roots(checkout);
    if roots
        .iter()
        .any(|root| resolved != *root && resolved.starts_with(root))
    {
        return Ok(resolved);
    }
    let reason = if roots.contains(&resolved) {
        "not below"
    } else {
        "outside"
    };
    Err(CiError::new(format!(
        "cache target {} is a symlink to {}, which is {reason} the mbx cache, RUNNER_TEMP and the checkout; refusing to follow it",
        link.display(),
        resolved.display()
    )))
}

fn mbx_setting(key: &str) -> Option<PathBuf> {
    let output = Command::new("mbx")
        .args(["settings", "get", key])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let text = String::from_utf8(output.stdout).ok()?;
    if text.is_empty() {
        return None;
    }
    let text = text.strip_suffix('\n').unwrap_or(&text);
    Some(PathBuf::from(text.strip_suffix('\r').unwrap_or(text)))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn setting(env: &str, key: &str) -> Option<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .or_else(|| mbx_setting(key))
}

fn default_cache_dir_for(
    macos: bool,
    home: Option<PathBuf>,
    xdg: Option<PathBuf>,
) -> Option<PathBuf> {
    if macos {
        return home.map(|home| home.join("Library/Caches/mbx"));
    }
    xdg.map(|xdg| xdg.join("mbx"))
        .or_else(|| home.map(|home| home.join(".cache/mbx")))
}

fn default_cache_dir() -> Option<PathBuf> {
    default_cache_dir_for(
        cfg!(target_os = "macos"),
        env_path("HOME"),
        env_path("XDG_CACHE_HOME"),
    )
}

fn allowed_roots(checkout: &Path) -> Vec<PathBuf> {
    let cache_dir = setting("MBX_CACHE_DIR", "cache_dir")
        .filter(|dir| !dir.as_os_str().is_empty())
        .or_else(default_cache_dir);
    let target_root = setting("MBX_TARGET_ROOT", "target.root").map(|root| match &cache_dir {
        Some(cache) => cache.join(&root),
        None => root,
    });
    let mut roots = vec![checkout.to_path_buf()];
    roots.extend(env_path("RUNNER_TEMP"));
    roots.extend(cache_dir.map(|dir| dir.join("targets")));
    roots.extend(target_root);
    let mut roots: Vec<PathBuf> = roots
        .into_iter()
        .filter(|root| root.is_absolute())
        .filter_map(|root| fs::canonicalize(root).ok())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    use super::default_cache_dir_for;
    use std::path::PathBuf;

    fn dir(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[test]
    fn linux_default_cache_dir_uses_xdg_without_home() {
        assert_eq!(
            default_cache_dir_for(false, None, Some(dir("/xdg"))),
            Some(dir("/xdg/mbx"))
        );
    }

    #[test]
    fn linux_default_cache_dir_prefers_xdg_over_home() {
        assert_eq!(
            default_cache_dir_for(false, Some(dir("/home/u")), Some(dir("/xdg"))),
            Some(dir("/xdg/mbx"))
        );
        assert_eq!(
            default_cache_dir_for(false, Some(dir("/home/u")), None),
            Some(dir("/home/u/.cache/mbx"))
        );
        assert_eq!(default_cache_dir_for(false, None, None), None);
    }

    #[test]
    fn macos_default_cache_dir_ignores_xdg() {
        assert_eq!(
            default_cache_dir_for(true, Some(dir("/Users/u")), Some(dir("/xdg"))),
            Some(dir("/Users/u/Library/Caches/mbx"))
        );
        assert_eq!(default_cache_dir_for(true, None, Some(dir("/xdg"))), None);
    }
}
