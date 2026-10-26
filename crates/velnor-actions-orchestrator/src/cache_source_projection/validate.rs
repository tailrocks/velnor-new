// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::types::HEX_DIGITS;
use crate::OrchestratorError;
use crate::internal::internal;

#[must_use]
pub fn is_git_path(path: &str) -> bool {
    path == ".git" || path.starts_with(".git/")
}

pub fn validate_relative_path(path: &str) -> Result<(), OrchestratorError> {
    if path.is_empty() || path.contains('\0') || path.contains('\\') {
        return Err(internal("projection_path_invalid"));
    }
    if path == "." || path.starts_with("./") || path.contains("/./") || path.ends_with("/.") {
        return Err(internal("projection_path_escape"));
    }
    if is_git_path(path) {
        return Err(internal("projection_git_path"));
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(internal("projection_path_absolute"));
    }
    let mut depth = 0_usize;
    for component in parsed.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir | Component::ParentDir => {
                return Err(internal("projection_path_escape"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(internal("projection_path_absolute"));
            }
        }
    }
    if depth == 0 {
        return Err(internal("projection_path_invalid"));
    }
    Ok(())
}

pub fn validate_link_target(target: &str) -> Result<(), OrchestratorError> {
    if target.is_empty() || target.contains('\0') || target.contains('\\') {
        return Err(internal("projection_link_invalid"));
    }
    let parsed = Path::new(target);
    if parsed.is_absolute() {
        return Err(internal("projection_link_absolute"));
    }
    let mut depth = 0_usize;
    let mut normal = false;
    for component in parsed.components() {
        match component {
            Component::Normal(_) => {
                depth += 1;
                normal = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                depth = depth.saturating_sub(1);
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(internal("projection_link_absolute"));
            }
        }
    }
    if !normal || depth == 0 {
        return Err(internal("projection_link_invalid"));
    }
    Ok(())
}

pub fn validate_link_target_in_root(
    root: &Path,
    link_path: &str,
    target: &str,
) -> Result<(), OrchestratorError> {
    validate_relative_path(link_path)?;
    validate_link_target(target)?;
    if !root.is_absolute() {
        return Err(internal("projection_root_not_absolute"));
    }
    let mut stack: Vec<String> = Path::new(link_path)
        .parent()
        .map(|parent| {
            parent
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    for component in Path::new(target).components() {
        match component {
            Component::Normal(part) => {
                stack.push(part.to_string_lossy().into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err(internal("projection_link_escape"));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(internal("projection_link_absolute"));
            }
        }
    }
    let mut joined = root.to_path_buf();
    for part in &stack {
        joined.push(part);
    }
    if !joined.starts_with(root) {
        return Err(internal("projection_link_escape"));
    }
    Ok(())
}

pub fn validate_sha256(value: &str) -> Result<(), OrchestratorError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(internal("projection_digest_invalid"));
    }
    Ok(())
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn staged_path(root: &Path, path: &str) -> Result<PathBuf, OrchestratorError> {
    validate_relative_path(path)?;
    if !root.is_absolute() {
        return Err(internal("projection_root_not_absolute"));
    }
    let joined = root.join(path);
    if !joined.starts_with(root) {
        return Err(internal("projection_path_escape"));
    }
    Ok(joined)
}

pub fn reject_link_parents(root: &Path, path: &str) -> Result<(), OrchestratorError> {
    validate_relative_path(path)?;
    let mut current = root.to_path_buf();
    let components: Vec<String> = Path::new(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    for component in &components[..components.len().saturating_sub(1)] {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(internal("projection_parent_symlink"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(internal("projection_parent_unreadable")),
        }
    }
    Ok(())
}
