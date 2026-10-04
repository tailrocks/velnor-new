//! Private checkout bytes bind every derivation to one verified inventory.

use std::fs;
use std::io::Read as _;
use std::path::{Component, Path};

use rustix::fs::{Mode, OFlags, open, openat};
use velnor_actions_contract::Provenance;
use velnor_actions_mise::command::OUTPUT_CAPTURE_LIMIT_BYTES;
use velnor_actions_rust::semantic_inputs::SemanticInventory;

use crate::OrchestratorError;
use crate::internal_plan::closure::checkout_inputs;

const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;

/// Owns an isolated exact copy until all current-source derivations finish.
pub(crate) struct FrozenCheckout {
    directory: tempfile::TempDir,
    inventory: SemanticInventory,
}

impl FrozenCheckout {
    /// Copy only known inventory; independently bind the copy before use.
    pub(crate) fn capture(root: &Path) -> Result<Self, OrchestratorError> {
        let inventory = known_inventory(root)?;
        Self::copy_inventory(root, inventory)
    }

    /// Every derivation reads this private root, never the mutable checkout.
    pub(crate) fn root(&self) -> &Path {
        self.directory.path()
    }

    /// Detect writes by derivation helpers before authorizing their result.
    pub(crate) fn verify_frozen(&self) -> Result<(), OrchestratorError> {
        require_same(&self.inventory, &known_inventory(self.root())?)
    }

    /// Refuse success if the caller's checkout no longer matches admission.
    pub(crate) fn verify_current(&self, root: &Path) -> Result<(), OrchestratorError> {
        let current = known_inventory(root)?;
        reject_excluded_files(root, &current)?;
        require_same(&self.inventory, &current)
    }

    fn copy_inventory(
        root: &Path,
        inventory: SemanticInventory,
    ) -> Result<Self, OrchestratorError> {
        reject_excluded_files(root, &inventory)?;
        let directory = tempfile::Builder::new()
            .prefix("velnor-current-source-")
            .tempdir()
            .map_err(|err| failed(format!("snapshot_create:{err}")))?;
        let root_fd = open(root, directory_flags(), Mode::empty())
            .map_err(|err| failed(format!("snapshot_root_unreadable_or_symlink:{err}")))?;
        let mut captured_bytes = 0_usize;
        for path in &inventory.paths {
            let (bytes, permissions) = read_input(&root_fd, path)?;
            captured_bytes = captured_bytes
                .checked_add(bytes.len())
                .filter(|total| *total <= MAX_SNAPSHOT_BYTES)
                .ok_or_else(|| failed("snapshot_aggregate_size_limit"))?;
            let destination = directory.path().join(path);
            let parent = destination
                .parent()
                .ok_or_else(|| failed("snapshot_path_without_parent"))?;
            fs::create_dir_all(parent)
                .map_err(|err| failed(format!("snapshot_parent_create:{err}")))?;
            fs::write(&destination, bytes)
                .map_err(|err| failed(format!("snapshot_write:{path}:{err}")))?;
            fs::set_permissions(&destination, permissions)
                .map_err(|err| failed(format!("snapshot_permissions:{path}:{err}")))?;
        }
        require_same(&inventory, &known_inventory(directory.path())?)?;
        Ok(Self {
            directory,
            inventory,
        })
    }
}

/// Unknown inventory cannot authorize copying or current-source reuse.
fn known_inventory(root: &Path) -> Result<SemanticInventory, OrchestratorError> {
    let inventory = checkout_inputs::collect(root);
    match &inventory.provenance {
        Provenance::Known { .. } => Ok(inventory),
        other => Err(failed(format!("checkout_inventory_unverified:{other:?}"))),
    }
}

fn require_same(
    expected: &SemanticInventory,
    actual: &SemanticInventory,
) -> Result<(), OrchestratorError> {
    if expected.paths != actual.paths || expected.provenance != actual.provenance {
        return Err(failed("checkout_changed_during_current_source_proof"));
    }
    Ok(())
}

/// Excluded first-party bytes cannot become implicitly absent in the copy.
fn reject_excluded_files(
    root: &Path,
    inventory: &SemanticInventory,
) -> Result<(), OrchestratorError> {
    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0_u32;
    while let Some(directory) = pending.pop() {
        for entry in
            fs::read_dir(&directory).map_err(|err| failed(format!("snapshot_walk:{err}")))?
        {
            let entry = entry.map_err(|err| failed(format!("snapshot_walk:{err}")))?;
            if directory == root && entry.file_name() == ".git" {
                continue;
            }
            visited += 1;
            if visited > 20_000 {
                return Err(failed("snapshot_walk_limit"));
            }
            let kind = entry
                .file_type()
                .map_err(|err| failed(format!("snapshot_file_type:{err}")))?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .map_err(|err| failed(format!("snapshot_path:{err}")))?
                    .to_str()
                    .ok_or_else(|| failed("snapshot_path_non_utf8"))?;
                if inventory
                    .paths
                    .binary_search_by(|path| path.as_str().cmp(relative))
                    .is_err()
                {
                    return Err(failed(format!("snapshot_excluded_input:{relative}")));
                }
            } else {
                return Err(failed("snapshot_symlink_or_nonregular_input"));
            }
        }
    }
    Ok(())
}

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

/// Pin every ancestor and leaf; read binary bytes and mode from one handle.
fn read_input(
    root: &rustix::fd::OwnedFd,
    path: &str,
) -> Result<(Vec<u8>, fs::Permissions), OrchestratorError> {
    if path.is_empty()
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(failed(format!("snapshot_unsafe_path:{path}")));
    }
    let mut directory =
        rustix::io::dup(root).map_err(|err| failed(format!("snapshot_root_handle:{err}")))?;
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_some() {
            directory = openat(&directory, component, directory_flags(), Mode::empty())
                .map_err(|err| failed(format!("snapshot_parent_unreadable:{path}:{err}")))?;
            continue;
        }
        let fd = openat(
            &directory,
            component,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|err| failed(format!("snapshot_input_unreadable:{path}:{err}")))?;
        let file = fs::File::from(fd);
        let metadata = file
            .metadata()
            .map_err(|err| failed(format!("snapshot_input_metadata:{path}:{err}")))?;
        if !metadata.is_file() {
            return Err(failed(format!("snapshot_nonregular_input:{path}")));
        }
        let limit = u64::try_from(OUTPUT_CAPTURE_LIMIT_BYTES)
            .map_err(|err| failed(format!("snapshot_input_limit:{err}")))?;
        if metadata.len() > limit {
            return Err(failed(format!("snapshot_oversized_input:{path}")));
        }
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| failed(format!("snapshot_input_read:{path}:{err}")))?;
        if bytes.len() > OUTPUT_CAPTURE_LIMIT_BYTES {
            return Err(failed(format!("snapshot_oversized_input:{path}")));
        }
        return Ok((bytes, metadata.permissions()));
    }
    Err(failed("snapshot_empty_path"))
}

fn failed(problem: impl Into<String>) -> OrchestratorError {
    crate::internal::internal(&format!(
        "helper_current_source_snapshot:{}",
        problem.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_binary_bytes_survive_live_mutation() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        fs::create_dir(root.path().join("nested"))?;
        let path = root.path().join("nested/input.bin");
        fs::write(&path, [0, 255, 254, 13])?;
        let snapshot = FrozenCheckout::capture(root.path())?;
        snapshot.verify_current(root.path())?;
        fs::write(&path, b"changed")?;
        assert_eq!(
            fs::read(snapshot.root().join("nested/input.bin"))?,
            [0, 255, 254, 13]
        );
        assert!(snapshot.verify_current(root.path()).is_err());
        fs::write(&path, [0, 255, 254, 13])?;
        snapshot.verify_current(root.path())?;
        fs::write(root.path().join("added"), b"new")?;
        assert!(snapshot.verify_current(root.path()).is_err());
        Ok(())
    }

    #[test]
    fn copy_must_match_previously_admitted_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        fs::write(root.path().join("input"), b"admitted")?;
        let inventory = known_inventory(root.path())?;
        fs::write(root.path().join("input"), b"substituted")?;
        assert!(FrozenCheckout::copy_inventory(root.path(), inventory).is_err());
        Ok(())
    }

    #[test]
    fn derivation_writes_in_frozen_root_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        fs::write(root.path().join("input"), b"admitted")?;
        let snapshot = FrozenCheckout::capture(root.path())?;
        snapshot.verify_frozen()?;
        fs::write(snapshot.root().join("input"), b"derived substitution")?;
        assert!(snapshot.verify_frozen().is_err());
        Ok(())
    }

    #[test]
    fn oversized_metadata_refused_before_read() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        let file = fs::File::create(root.path().join("oversized"))?;
        file.set_len(u64::try_from(OUTPUT_CAPTURE_LIMIT_BYTES)? + 1)?;
        let fd = open(root.path(), directory_flags(), Mode::empty())?;
        let error = read_input(&fd, "oversized").expect_err("oversized input must fail");
        assert!(error.to_string().contains("snapshot_oversized_input"));
        Ok(())
    }

    #[test]
    fn excluded_input_and_traversal_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        fs::write(root.path().join("input"), b"admitted")?;
        let inventory = known_inventory(root.path())?;
        fs::write(root.path().join("excluded"), b"unadmitted")?;
        assert!(reject_excluded_files(root.path(), &inventory).is_err());
        let fd = open(root.path(), directory_flags(), Mode::empty())?;
        for path in [
            "../input",
            "/input",
            "./input",
            "nested//input",
            "nested\\input",
            "",
        ] {
            assert!(read_input(&fd, path).is_err(), "accepted {path}");
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn copied_permissions_and_changes_are_bound() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::TempDir::new()?;
        let path = root.path().join("script");
        fs::write(&path, b"script")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o751))?;
        let snapshot = FrozenCheckout::capture(root.path())?;
        assert_eq!(
            fs::metadata(snapshot.root().join("script"))?
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert!(snapshot.verify_current(root.path()).is_err());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn pinned_reader_refuses_symlink_leaf_parent_and_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        fs::create_dir(root.path().join("actual"))?;
        fs::write(root.path().join("actual/input"), b"bytes")?;
        std::os::unix::fs::symlink("actual/input", root.path().join("leaf"))?;
        std::os::unix::fs::symlink("actual", root.path().join("parent"))?;
        let fd = open(root.path(), directory_flags(), Mode::empty())?;
        assert!(read_input(&fd, "leaf").is_err());
        assert!(read_input(&fd, "parent/input").is_err());
        assert!(read_input(&fd, "actual").is_err());
        assert!(FrozenCheckout::capture(root.path()).is_err());
        Ok(())
    }
}
