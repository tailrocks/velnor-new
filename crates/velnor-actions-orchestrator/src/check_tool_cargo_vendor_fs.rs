//! Filesystem custody for verified Cargo source archives.
use crate::OrchestratorError;
use crate::internal::internal;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_mise::CheckDeadline;

fn io(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

pub(super) fn require_absent(path: &Path) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io(path, error)),
        Ok(_) => Err(internal("qualified_cargo_destination_exists")),
    }
}

pub(super) fn verify_owned_root(home: &Path, root: &Path) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| io(root, error))?;
    let home = home.canonicalize().map_err(|error| io(home, error))?;
    let canonical = root.canonicalize().map_err(|error| io(root, error))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || canonical != root
        || !canonical.starts_with(home.join("unpacked"))
    {
        return Err(internal("qualified_cargo_source_not_owned"));
    }
    reject_ancestor_configs(root)
}

pub(super) fn reject_ancestor_configs(path: &Path) -> Result<(), OrchestratorError> {
    for ancestor in path.ancestors() {
        let cargo = ancestor.join(".cargo");
        match fs::symlink_metadata(&cargo) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(internal("qualified_cargo_ancestor_config"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io(&cargo, error)),
        }
        for name in ["config", "config.toml"] {
            match fs::symlink_metadata(cargo.join(name)) {
                Ok(_) => return Err(internal("qualified_cargo_ancestor_config")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io(&cargo, error)),
            }
        }
    }
    Ok(())
}

pub(super) fn crate_root(root: &Path) -> Result<PathBuf, OrchestratorError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| io(root, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(internal("qualified_cargo_crate_root"));
    }
    if root.join("Cargo.toml").is_file() {
        return Ok(root.to_path_buf());
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| io(root, error))? {
        let entry = entry.map_err(|error| io(root, error))?;
        let kind = entry.file_type().map_err(|error| io(root, error))?;
        if kind.is_dir() && entry.path().join("Cargo.toml").is_file() {
            candidates.push(entry.path());
        }
    }
    match candidates.as_slice() {
        [root] => Ok(root.clone()),
        _ => Err(internal("qualified_cargo_ambiguous_crate_root")),
    }
}

pub(super) fn inspect_tree(root: &Path, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    file_hashes(root, deadline).map(|_| ())
}

fn file_hashes(
    root: &Path,
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0usize;
    let mut bytes = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| io(&directory, error))? {
            check_deadline(deadline)?;
            let path = entry.map_err(|error| io(&directory, error))?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| io(&path, error))?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| internal("qualified_cargo_tree_escape"))?;
            let name = relative
                .to_str()
                .ok_or_else(|| internal("qualified_cargo_non_utf8"))?;
            entries += 1;
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or_else(|| internal("qualified_cargo_tree_budget"))?;
            if entries > 100_000
                || bytes > 512 * 1024 * 1024
                || relative.components().count() > 64
                || metadata.file_type().is_symlink()
                || (!metadata.is_dir() && !metadata.is_file())
            {
                return Err(internal("qualified_cargo_unsafe_tree"));
            }
            if name == ".cargo-checksum.json"
                || name.ends_with("/.cargo-checksum.json")
                || relative.ends_with(".cargo/config")
                || relative.ends_with(".cargo/config.toml")
            {
                return Err(internal("qualified_cargo_source_config"));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else {
                let content = crate::retrieve_reports::staged_reads::read_staged_bytes_until(
                    &path,
                    512 * 1024 * 1024,
                    || deadline.remaining().map(|_| ()).map_err(|_| "deadline"),
                )
                .map_err(|problem| {
                    if problem == "deadline" {
                        internal("check_timeout:deadline_exhausted")
                    } else {
                        internal("qualified_cargo_source_file")
                    }
                })?;
                files.insert(name.to_owned(), sha256_with_deadline(&content, deadline)?);
            }
        }
    }
    Ok(files)
}

pub(super) fn write_checksum(
    root: &Path,
    package: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let files = file_hashes(root, deadline)?;
    let bytes = serde_json::to_vec(&serde_json::json!({"files":files,"package":package}))
        .map_err(|error| internal(&format!("qualified_cargo_checksum:{error}")))?;
    check_deadline(deadline)?;
    crate::exclusive_write::write_exclusive_until(
        &root.join(".cargo-checksum.json"),
        &bytes,
        "qualified_cargo_checksum",
        || check_deadline(deadline),
    )
}

fn sha256_with_deadline(
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(64 * 1024) {
        check_deadline(deadline)?;
        hash.update(chunk);
    }
    let digest = hash.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").map_err(|_| internal("qualified_cargo_sha256"))?;
    }
    Ok(hex)
}

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}
