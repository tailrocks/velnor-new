//! Action source/output admission is separate from MBX tool distribution admission.

use crate::MiseError;
use sha2::{Digest, Sha256};

/// Closed action grant for strict preinstalled comparison export and its outputs.
/// No existing official action pin or qualified MBX executable constructs this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedMbxAction {
    source_repository: &'static str,
    source_commit: &'static str,
    source_tree: &'static str,
    release_version: &'static str,
    dist_sha256: &'static str,
    behavior_abi: &'static str,
    output_abi: &'static str,
}

impl QualifiedMbxAction {
    /// Require the published and reviewed preinstalled comparison/output action ABI.
    /// # Errors
    /// No reviewed publication binds the revised action source and compiled distribution.
    pub fn require_comparison_export() -> Result<Self, MiseError> {
        Err(MiseError::Contract {
            problem: "MBX comparison export action qualification absent: reviewed published source/distribution/behavior/output ABI required".to_owned(),
        })
    }
    /// Immutable action repository identity.
    #[must_use]
    pub const fn source_repository(self) -> &'static str {
        self.source_repository
    }
    /// Actual published action source commit.
    #[must_use]
    pub const fn source_commit(self) -> &'static str {
        self.source_commit
    }
    /// Exact complete action source tree.
    #[must_use]
    pub const fn source_tree(self) -> &'static str {
        self.source_tree
    }
    /// Actual release version, never an alias for the old official action.
    #[must_use]
    pub const fn release_version(self) -> &'static str {
        self.release_version
    }
    /// Compiled action distribution digest at the source commit.
    #[must_use]
    pub const fn dist_sha256(self) -> &'static str {
        self.dist_sha256
    }
    /// Qualified preinstalled/export behavior contract.
    #[must_use]
    pub const fn behavior_abi(self) -> &'static str {
        self.behavior_abi
    }
    /// Qualified complete output contract.
    #[must_use]
    pub const fn output_abi(self) -> &'static str {
        self.output_abi
    }
    /// Canonical action authority identity; tool qualification remains separate.
    #[must_use]
    pub fn qualification_digest(self) -> String {
        let mut digest = Sha256::new();
        for field in [
            "velnor-mbx-action-qualification-v1",
            self.source_repository,
            self.source_commit,
            self.source_tree,
            self.release_version,
            self.dist_sha256,
            self.behavior_abi,
            self.output_abi,
        ] {
            digest.update(field.len().to_string().as_bytes());
            digest.update(b":");
            digest.update(field.as_bytes());
        }
        let digest = digest.finalize();
        encode_hex(&digest)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
#[path = "catalog_mbx_action_authority_tests.rs"]
mod tests;
