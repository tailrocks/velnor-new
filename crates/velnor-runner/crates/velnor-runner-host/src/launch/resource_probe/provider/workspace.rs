//! Private workspaces for downloaded and validated release material.

use std::fs::{self, DirBuilder};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::ProviderFailure;

const WORKSPACE_DIGEST_HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) struct Workspace {
    root: PathBuf,
    assets: PathBuf,
    cleanup_verified: bool,
}

/// Evidence that the private workspace is absent after cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::launch) struct VerifiedWorkspaceCleanup {
    workspace_digest: String,
}

impl VerifiedWorkspaceCleanup {
    pub(in crate::launch) fn workspace_digest(&self) -> &str {
        &self.workspace_digest
    }

    #[cfg(test)]
    pub(in crate::launch) fn test_fixture() -> Self {
        Self::test_fixture_with_digest("d".repeat(64))
    }

    #[cfg(test)]
    pub(in crate::launch) fn test_fixture_with_digest(digest: impl Into<String>) -> Self {
        Self {
            workspace_digest: digest.into(),
        }
    }
}

impl Workspace {
    pub(super) fn create_native() -> Result<Self, ProviderFailure> {
        let root = std::env::temp_dir().join(format!("velnor-resource-probe-{}", Uuid::new_v4()));
        create_private_dir(&root)?;
        let assets = root.join("assets");
        if create_private_dir(&assets).is_err() {
            remove_workspace(&root);
            return Err(ProviderFailure::WorkspaceCreate);
        }
        Ok(Self {
            root,
            assets,
            cleanup_verified: false,
        })
    }

    pub(super) fn assets(&self) -> &Path {
        &self.assets
    }

    pub(super) fn remove_verified(mut self) -> Result<VerifiedWorkspaceCleanup, ProviderFailure> {
        let workspace_digest = workspace_digest(&self.root);
        remove_workspace_verified(&self.root)?;
        self.cleanup_verified = true;
        Ok(VerifiedWorkspaceCleanup { workspace_digest })
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if !self.cleanup_verified {
            remove_workspace(&self.root);
        }
    }
}

fn remove_workspace_verified(path: &Path) -> Result<(), ProviderFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProviderFailure::WorkspaceCleanup)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ProviderFailure::WorkspaceCleanup);
    }
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ProviderFailure::WorkspaceCleanup),
    }
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(ProviderFailure::WorkspaceCleanup),
    }
}

fn workspace_digest(path: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-release-workspace-cleanup-v1\\0");
    let encoded = path.as_os_str().to_string_lossy();
    digest.update((encoded.len() as u64).to_be_bytes());
    digest.update(encoded.as_bytes());
    let mut result = String::with_capacity(64);
    for byte in digest.finalize() {
        result.push(char::from(WORKSPACE_DIGEST_HEX[usize::from(byte >> 4)]));
        result.push(char::from(WORKSPACE_DIGEST_HEX[usize::from(byte & 0x0f)]));
    }
    result
}

fn remove_workspace(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        eprintln!("resource_probe=workspace_cleanup_failed reason={error}");
    }
}

fn create_private_dir(path: &Path) -> Result<(), ProviderFailure> {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|_| ProviderFailure::WorkspaceCreate)
}

#[cfg(test)]
mod tests {
    use super::{ProviderFailure, Workspace};

    #[test]
    fn verified_workspace_cleanup_returns_only_after_exact_root_is_absent() -> Result<(), String> {
        let workspace = Workspace::create_native().map_err(|error| format!("{error:?}"))?;
        let root = workspace.root.clone();
        let receipt = workspace
            .remove_verified()
            .map_err(|error| format!("The verified cleanup failed. Error: {error:?}."))?;
        if root.exists() || receipt.workspace_digest().len() != 64 {
            return Err("The cleanup receipt is invalid or the workspace still exists.".to_owned());
        }
        Ok(())
    }

    #[test]
    fn replaced_workspace_root_does_not_produce_cleanup_receipt() -> Result<(), String> {
        let workspace = Workspace::create_native().map_err(|error| format!("{error:?}"))?;
        let root = workspace.root.clone();
        std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        std::fs::write(&root, b"replacement").map_err(|error| error.to_string())?;
        let result = workspace.remove_verified();
        assert_eq!(result.err(), Some(ProviderFailure::WorkspaceCleanup));
        std::fs::remove_file(root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
