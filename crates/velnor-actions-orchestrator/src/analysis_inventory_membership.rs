//! Source-qualified Cargo workspace expansion inputs, never member metadata.
//!
//! Cargo 797e8a9bc workspace.rs uses glob 0.3.3 default options, counts raw
//! matches before filtering directories, and falls back to the literal pattern
//! only when there are zero raw matches. Bind every match, including excluded
//! directories, without reimplementing Cargo membership or exclusion rules.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use velnor_actions_contract::digest_b3;
use velnor_actions_rust::WorkspaceRecord;

use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

const MAX_MATCHES: usize = 20000;

#[derive(Debug, Serialize)]
pub(super) struct Expansion {
    workspace_root: String,
    field: String,
    pattern: String,
    literal_fallback: bool,
    parent: Match,
    matches: Vec<Match>,
}

#[derive(Debug, Serialize)]
struct Match {
    path: String,
    kind: NodeKind,
    manifest: Option<String>,
}

#[derive(Debug, Serialize)]
enum NodeKind {
    Absent,
    File,
    Directory,
}

/// Bind only the scopes Cargo actually expands. Opaque/unsafe patterns fall
/// back to fresh Cargo; no filesystem-wide admin/output inventory is included.
pub(super) fn capture(
    root: &Path,
    inventories: &[(String, WorkspaceRecord)],
) -> Result<Vec<Expansion>, String> {
    let roots: BTreeSet<_> = inventories
        .iter()
        .map(|(_, record)| record.workspace_root.as_str())
        .collect();
    let mut expansions = Vec::new();
    for workspace_root in roots {
        let manifest = crate::discover::workspace_manifest(workspace_root);
        let text = match read_repo_file(root, &manifest, MAX_REPO_FILE_BYTES)
            .map_err(|err| err.to_string())?
        {
            RepoRead::Text(text) => text,
            RepoRead::Absent => {
                return Err("analysis_inventory_missing_workspace_manifest".to_owned());
            }
        };
        let manifest: toml::Table = toml::from_str(&text).map_err(|err| err.to_string())?;
        let Some(workspace) = manifest.get("workspace") else {
            continue;
        };
        for field in ["members", "default-members"] {
            let Some(patterns) = workspace.get(field) else {
                continue;
            };
            let patterns = patterns
                .as_array()
                .ok_or("analysis_inventory_invalid_member_patterns")?;
            for pattern in patterns {
                let pattern = pattern
                    .as_str()
                    .ok_or("analysis_inventory_invalid_member_pattern")?;
                expansions.push(expand(root, workspace_root, field, pattern)?);
                if expansions.len() > MAX_MATCHES {
                    return Err("analysis_inventory_expansion_limit".to_owned());
                }
            }
        }
    }
    Ok(expansions)
}

fn expand(
    root: &Path,
    workspace_root: &str,
    field: &str,
    pattern: &str,
) -> Result<Expansion, String> {
    qualify_pattern(pattern)?;
    let relative = if pattern == "." {
        PathBuf::from(workspace_root)
    } else {
        Path::new(workspace_root).join(pattern)
    };
    let parent = relative.parent().unwrap_or(Path::new(""));
    reject_symlink_ancestors(root, parent)?;
    if pattern.contains('*') {
        qualify_expansion_directory(&root.join(parent))?;
    }
    let parent = bind_match(root, &root.join(parent))?;
    let full = root.join(&relative);
    let full = full
        .to_str()
        .ok_or("analysis_inventory_member_path_non_utf8")?;
    if root
        .to_str()
        .ok_or("analysis_inventory_member_path_non_utf8")?
        .contains(['*', '?', '[', ']'])
    {
        return Err("analysis_inventory_opaque_checkout_glob_path".to_owned());
    }
    let raw = glob::glob(full).map_err(|err| err.to_string())?;
    let mut matches = Vec::new();
    for matched in raw {
        let matched = matched.map_err(|err| err.to_string())?;
        matches.push(bind_match(root, &matched)?);
        if matches.len() > MAX_MATCHES {
            return Err("analysis_inventory_member_match_limit".to_owned());
        }
    }
    let literal_fallback = matches.is_empty();
    if literal_fallback {
        matches.push(bind_match(root, &root.join(relative))?);
    }
    matches.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Expansion {
        workspace_root: workspace_root.to_owned(),
        field: field.to_owned(),
        pattern: pattern.to_owned(),
        literal_fallback,
        parent,
        matches,
    })
}

fn qualify_expansion_directory(full: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(full) {
        Ok(metadata) => metadata,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(());
        }
        Err(err) => return Err(err.to_string()),
    };
    if metadata.is_dir() {
        for (index, entry) in std::fs::read_dir(full)
            .map_err(|err| err.to_string())?
            .enumerate()
        {
            if index >= MAX_MATCHES {
                return Err("analysis_inventory_member_match_limit".to_owned());
            }
            if entry
                .map_err(|err| err.to_string())?
                .file_name()
                .to_str()
                .is_none()
            {
                return Err("analysis_inventory_member_path_non_utf8".to_owned());
            }
        }
    }
    Ok(())
}

fn qualify_pattern(pattern: &str) -> Result<(), String> {
    if pattern == "." {
        return Ok(());
    }
    if pattern.is_empty()
        || Path::new(pattern).is_absolute()
        || pattern.contains(['\\', '?', '[', ']', '\0'])
    {
        return Err("analysis_inventory_unsupported_member_pattern".to_owned());
    }
    let parts: Vec<_> = pattern.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || matches!(*part, "." | ".."))
        || matches!(parts[0], ".git" | "target" | ".velnor")
    {
        return Err("analysis_inventory_unsafe_member_scope".to_owned());
    }
    let stars = pattern.bytes().filter(|byte| *byte == b'*').count();
    if stars > 1
        || stars == 1
            && (parts.len() < 2
                || parts[..parts.len() - 1]
                    .iter()
                    .any(|part| part.contains('*')))
    {
        return Err("analysis_inventory_unsupported_member_pattern".to_owned());
    }
    Ok(())
}

fn reject_symlink_ancestors(root: &Path, relative: &Path) -> Result<(), String> {
    for prefix in relative.ancestors() {
        match std::fs::symlink_metadata(root.join(prefix)) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err("analysis_inventory_member_symlink".to_owned());
            }
            Ok(_) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(())
}

fn bind_match(root: &Path, full: &Path) -> Result<Match, String> {
    let relative = full
        .strip_prefix(root)
        .map_err(|_| "analysis_inventory_member_root_escape")?;
    let path = relative
        .to_str()
        .ok_or("analysis_inventory_member_path_non_utf8")?
        .to_owned();
    reject_symlink_ancestors(root, relative)?;
    let kind = match std::fs::symlink_metadata(full) {
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            NodeKind::Absent
        }
        Err(err) => return Err(err.to_string()),
        Ok(metadata) if metadata.is_file() => NodeKind::File,
        Ok(metadata) if metadata.is_dir() => NodeKind::Directory,
        Ok(_) => return Err("analysis_inventory_member_node_kind".to_owned()),
    };
    let manifest = if matches!(kind, NodeKind::Directory) {
        let manifest = relative.join("Cargo.toml");
        let manifest = manifest
            .to_str()
            .ok_or("analysis_inventory_member_path_non_utf8")?;
        match read_repo_file(root, manifest, MAX_REPO_FILE_BYTES).map_err(|err| err.to_string())? {
            RepoRead::Text(text) => Some(digest_b3(text.as_bytes())),
            RepoRead::Absent => None,
        }
    } else {
        None
    };
    Ok(Match {
        path,
        kind,
        manifest,
    })
}

#[cfg(test)]
#[path = "analysis_inventory_membership_tests.rs"]
mod tests;
