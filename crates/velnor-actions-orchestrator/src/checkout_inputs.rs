//! Conservative first-party semantic inputs, independent of filename classes.
//!
//! Every tracked file can be read by Rust macros, native tooling, or fixtures.
//! Narrower closure requires explicit complete input evidence. Generated local
//! build output is excluded by the Git inventory, never by filename guessing.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use velnor_actions_contract::{Provenance, digest_b3, normalize_posix_path};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::semantic_inputs::SemanticInventory;

/// Resolve all first-party bytes once; unreadable/incomplete inventory is unknown.
#[cfg(test)]
pub(crate) fn resolve(root: &Path) -> Provenance {
    collect(root).provenance
}

/// Inventory and bound content are a single immutable planning snapshot.
pub(crate) fn collect(root: &Path) -> SemanticInventory {
    let files = match inventory(root) {
        Ok(files) => files,
        Err(reason) => {
            return SemanticInventory {
                paths: Vec::new(),
                provenance: Provenance::Unknown { reason },
            };
        }
    };
    let provenance = match resolve_digest(root, &files) {
        Ok(digest) => Provenance::Known { digest },
        Err(reason) => Provenance::Unknown { reason },
    };
    SemanticInventory {
        paths: files.into_iter().collect(),
        provenance,
    }
}

/// Hash sorted path, actual permissions, and contents, preserving local edits.
fn resolve_digest(root: &Path, files: &BTreeSet<String>) -> Result<String, String> {
    if files.is_empty() {
        return Err("empty_checkout_inventory".to_owned());
    }
    let mut inputs = Vec::new();
    for path in files {
        let normalized = normalize_posix_path(&path).map_err(|err| err.to_string())?;
        if normalized != *path {
            return Err(format!("noncanonical_checkout_path:{path}"));
        }
        let full = checked_file(root, &path)?;
        let metadata = std::fs::symlink_metadata(&full).map_err(|err| err.to_string())?;
        let bytes = bounded_bytes(&full).map_err(|err| format!("unreadable:{path}:{err}"))?;
        inputs.push((path, permissions(&metadata), digest_b3(&bytes)));
    }
    super::super::snapshot::canonical_digest(&inputs).map_err(|err| err.to_string())
}

/// Semantic inventories share the transport's finite per-file capture bound.
fn bounded_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let bound = velnor_actions_mise::command::OUTPUT_CAPTURE_LIMIT_BYTES as u64;
    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let metadata = file.metadata().map_err(|err| err.to_string())?;
    if !metadata.is_file() || metadata.len() > bound {
        return Err("checkout_input_capture_limit".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(bound.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if bytes.len() as u64 > bound {
        return Err("checkout_input_capture_limit".to_owned());
    }
    Ok(bytes)
}

/// Match the exact root; cached mode/stage rejects submodules and conflicts.
fn inventory(root: &Path) -> Result<BTreeSet<String>, String> {
    let output = GitRequest::rev_parse(vec!["--show-toplevel".into()])
        .run_in(root)
        .map_err(|err| format!("git_inventory:{err}"))?;
    if !output.success {
        if root.join(".git").exists() {
            return Err("git_inventory_failed".to_owned());
        }
        return fallback_files(root);
    }
    let top = String::from_utf8(output.stdout).map_err(|_| "git_root_non_utf8")?;
    let actual =
        std::fs::canonicalize(top.trim_end_matches(['\n', '\r'])).map_err(|err| err.to_string())?;
    let expected = std::fs::canonicalize(root).map_err(|err| err.to_string())?;
    if actual != expected {
        return Err("checkout_root_mismatch".to_owned());
    }
    let output = GitRequest::ls_files(vec!["--stage".into(), "-z".into()])
        .run_in(root)
        .map_err(|err| format!("git_inventory:{err}"))?;
    if !output.success {
        return Err("git_inventory_failed".to_owned());
    }
    let mut files = parse_inventory(&output.stdout)?;
    let untracked = GitRequest::ls_files(vec![
        "--others".into(),
        "--exclude-standard".into(),
        "-z".into(),
    ])
    .run_in(root)
    .map_err(|err| format!("git_inventory:{err}"))?;
    if !untracked.success {
        return Err("git_inventory_failed".to_owned());
    }
    let paths: BTreeSet<String> = crate::git_paths::split_nul_paths(&untracked.stdout)?;
    files.extend(paths);
    Ok(files)
}

/// NUL records preserve paths; stage zero regular files are the qualified set.
fn parse_inventory(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let text = std::str::from_utf8(record).map_err(|_| "checkout_path_non_utf8")?;
        let (header, path) = text.split_once('\t').ok_or("invalid_index_record")?;
        let mut fields = header.split(' ');
        let mode = fields.next().ok_or("invalid_index_mode")?;
        let _object = fields.next().ok_or("invalid_index_object")?;
        let stage = fields.next().ok_or("invalid_index_stage")?;
        if fields.next().is_some() || stage != "0" {
            return Err(format!("unresolved_index:{path}"));
        }
        if !matches!(mode, "100644" | "100755") {
            return Err(format!("unsupported_checkout_kind:{path}:{mode}"));
        }
        if !files.insert(path.to_owned()) {
            return Err(format!("duplicate_checkout_path:{path}"));
        }
    }
    Ok(files)
}

/// Non-Git fixtures: strict full tree, with no inferred semantic exclusions.
fn fallback_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = BTreeSet::new();
    let mut directories = 0;
    while let Some(dir) = stack.pop() {
        directories += 1;
        if directories > 20000 {
            return Err("checkout_directory_limit".to_owned());
        }
        for entry in std::fs::read_dir(&dir).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let ty = entry.file_type().map_err(|err| err.to_string())?;
            if ty.is_symlink() {
                return Err(format!("symlink:{}", entry.path().display()));
            }
            if ty.is_dir() {
                stack.push(entry.path());
            } else if ty.is_file() {
                let full = entry.path();
                let path = full.strip_prefix(root).map_err(|err| err.to_string())?;
                let path = path.to_str().ok_or("checkout_path_non_utf8")?;
                if path.contains('\\') {
                    return Err("noncanonical_checkout_path".to_owned());
                }
                files.insert(path.to_owned());
            } else {
                return Err("unsupported_checkout_kind".to_owned());
            }
            if files.len() > 20000 {
                return Err("checkout_input_limit".to_owned());
            }
        }
    }
    Ok(files)
}

/// Refuse symlink ancestors as well as the leaf; never resolve external bytes.
fn checked_file(root: &Path, path: &str) -> Result<PathBuf, String> {
    let mut full = root.to_path_buf();
    for component in Path::new(path).components() {
        full.push(component);
        let meta = std::fs::symlink_metadata(&full).map_err(|err| err.to_string())?;
        if meta.file_type().is_symlink() {
            return Err(format!("symlink:{path}"));
        }
    }
    if !full.is_file() {
        return Err(format!("unsupported_checkout_kind:{path}"));
    }
    Ok(full)
}

/// Permissions include executable fixture inputs; platform identity is separate.
#[cfg(unix)]
fn permissions(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn permissions(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(test)]
#[path = "checkout_inputs_tests.rs"]
mod tests;
