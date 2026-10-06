//! Cold SDK Foundation qualification, separate from compiler and SDK genesis rights.

use crate::MiseError;
use sha2::{Digest, Sha256};

/// Opaque adopted root Linux source/artifact/behavior qualification.
/// No source hash, current-run witness, JSON, environment or public constructor grants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootLinuxColdFoundation {
    source_receipt_sha256: &'static str,
    artifact_receipt_sha256: &'static str,
    behavior_receipt_sha256: &'static str,
    installer_identity: &'static str,
    mise_qualification_sha256: &'static str,
}

impl RootLinuxColdFoundation {
    /// Require the exact adopted fresh-host Foundation supplier qualification.
    /// # Errors
    /// Rejects until actual source, artifact and behavior receipts have been qualified.
    pub fn require() -> Result<Self, MiseError> {
        Err(MiseError::Contract {
            problem: "root Linux cold Foundation qualification absent: adopted source/artifact/behavior receipts and fixed installer identity required".to_owned(),
        })
    }
    /// Fixed receipt schema.
    #[must_use]
    pub const fn schema(self) -> u32 {
        1
    }
    /// Foundation-only purpose; never compiler execution or SDK genesis authority.
    #[must_use]
    pub const fn purpose(self) -> &'static str {
        "source-intent-cold-sdk-foundation"
    }
    /// Exact native Foundation host.
    #[must_use]
    pub const fn host(self) -> &'static str {
        "x86_64-unknown-linux-gnu"
    }
    /// Fixed source filesystem runtime contract.
    #[must_use]
    pub const fn runtime_abi(self) -> &'static str {
        "source-original-fs-v3"
    }
    /// Adopted source qualification receipt, not a source-file digest proxy.
    #[must_use]
    pub const fn source_receipt_sha256(self) -> &'static str {
        self.source_receipt_sha256
    }
    /// Actual artifact/closure qualification receipt.
    #[must_use]
    pub const fn artifact_receipt_sha256(self) -> &'static str {
        self.artifact_receipt_sha256
    }
    /// Actual fresh-host supplier/behavior qualification receipt.
    #[must_use]
    pub const fn behavior_receipt_sha256(self) -> &'static str {
        self.behavior_receipt_sha256
    }
    /// Exact independently qualified source installer recipe identity.
    #[must_use]
    pub const fn installer_identity(self) -> &'static str {
        self.installer_identity
    }
    /// Exact Mise qualification bound by the adopted supplier recipe.
    #[must_use]
    pub const fn mise_qualification_sha256(self) -> &'static str {
        self.mise_qualification_sha256
    }
    /// Canonical identity of all qualification predicates and fixed purpose metadata.
    #[must_use]
    pub fn qualification_sha256(self) -> String {
        let mut digest = Sha256::new();
        for field in [
            "velnor-root-cold-foundation-v1",
            "1",
            self.purpose(),
            self.host(),
            self.runtime_abi(),
            self.source_receipt_sha256,
            self.artifact_receipt_sha256,
            self.behavior_receipt_sha256,
            self.installer_identity,
            self.mise_qualification_sha256,
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
#[path = "catalog_cold_foundation_tests.rs"]
mod tests;
