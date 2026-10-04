//! Compiler supply-chain admission, separate from a current-run filesystem witness.

use crate::MiseError;
use crate::source_intent_cold_root::SourceIntentColdRoot;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[path = "catalog_rust_compiler_manifest.rs"]
mod records;

#[path = "catalog_root_rust_candidate_source.rs"]
mod candidate_source;
pub use candidate_source::RootRustCandidateSource;

/// Official component transport; it does not authorize installed executable bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct RustComponentArtifact {
    component: &'static str,
    xz_url: &'static str,
    xz_sha256: &'static str,
    gzip_url: &'static str,
    gzip_sha256: &'static str,
}

impl RustComponentArtifact {
    /// Exact official manifest package name.
    #[must_use]
    pub const fn component(self) -> &'static str {
        self.component
    }
    /// Official dated XZ archive URL.
    #[must_use]
    pub const fn xz_url(self) -> &'static str {
        self.xz_url
    }
    /// XZ archive digest from the authenticated manifest.
    #[must_use]
    pub const fn xz_sha256(self) -> &'static str {
        self.xz_sha256
    }
    /// Official dated gzip archive URL.
    #[must_use]
    pub const fn gzip_url(self) -> &'static str {
        self.gzip_url
    }
    /// Gzip archive digest from the authenticated manifest.
    #[must_use]
    pub const fn gzip_sha256(self) -> &'static str {
        self.gzip_sha256
    }
}

/// Closed root Linux compiler manifest audit, without installation capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct OfficialRootRustManifest {
    version: &'static str,
    target: &'static str,
    manifest_url: &'static str,
    manifest_sha256: &'static str,
    release_date: &'static str,
    rust_source_repository: &'static str,
    rust_source_commit: &'static str,
    rust_source_tree: &'static str,
    cargo_source_repository: &'static str,
    cargo_source_commit: &'static str,
    cargo_source_tree: &'static str,
    components: &'static [RustComponentArtifact],
}

impl OfficialRootRustManifest {
    /// Exact official release metadata. This cannot become installed authority.
    #[must_use]
    pub const fn audit() -> Self {
        records::MANIFEST
    }
    /// Selected compiler version, distinct from Cargo's internal crate version.
    #[must_use]
    pub const fn version(self) -> &'static str {
        self.version
    }
    /// Actual native target, never a caller-supplied cross target.
    #[must_use]
    pub const fn target(self) -> &'static str {
        self.target
    }
    /// Versioned official manifest URL.
    #[must_use]
    pub const fn manifest_url(self) -> &'static str {
        self.manifest_url
    }
    /// Measured manifest digest, checked against its official sidecar.
    #[must_use]
    pub const fn manifest_sha256(self) -> &'static str {
        self.manifest_sha256
    }
    /// Official publication date.
    #[must_use]
    pub const fn release_date(self) -> &'static str {
        self.release_date
    }
    /// Official Rust source repository.
    #[must_use]
    pub const fn rust_source_repository(self) -> &'static str {
        self.rust_source_repository
    }
    /// Official Rust release source commit.
    #[must_use]
    pub const fn rust_source_commit(self) -> &'static str {
        self.rust_source_commit
    }
    /// Official Rust release source tree.
    #[must_use]
    pub const fn rust_source_tree(self) -> &'static str {
        self.rust_source_tree
    }
    /// Cargo gitlink source repository.
    #[must_use]
    pub const fn cargo_source_repository(self) -> &'static str {
        self.cargo_source_repository
    }
    /// Exact Cargo gitlink at the Rust source commit.
    #[must_use]
    pub const fn cargo_source_commit(self) -> &'static str {
        self.cargo_source_commit
    }
    /// Complete Cargo source tree at the verified gitlink.
    #[must_use]
    pub const fn cargo_source_tree(self) -> &'static str {
        self.cargo_source_tree
    }
    /// Minimal native compiler plus the root role's clippy/rustfmt components.
    #[must_use]
    pub const fn components(self) -> &'static [RustComponentArtifact] {
        self.components
    }
    /// Canonical manifest/component transport identity, without a runtime grant.
    #[must_use]
    pub fn qualification_digest(self) -> String {
        let mut digest = Sha256::new();
        let mut bind = |value: &str| {
            digest.update(value.len().to_string().as_bytes());
            digest.update(b":");
            digest.update(value.as_bytes());
        };
        for value in [
            "velnor-root-rust-manifest-v1",
            self.version,
            self.target,
            self.manifest_url,
            self.manifest_sha256,
            self.release_date,
            self.rust_source_repository,
            self.rust_source_commit,
            self.rust_source_tree,
            self.cargo_source_repository,
            self.cargo_source_commit,
            self.cargo_source_tree,
        ] {
            bind(value);
        }
        bind(&self.components.len().to_string());
        for component in self.components {
            for value in [
                component.component,
                component.xz_url,
                component.xz_sha256,
                component.gzip_url,
                component.gzip_sha256,
            ] {
                bind(value);
            }
        }
        let digest = digest.finalize();
        encode_hex(&digest)
    }
}

/// Opaque grant requiring official component bytes and qualified native installation.
/// No JSON, environment, version probe, or observed self-hash constructs this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RustCompilerArtifactAuthority {
    manifest: OfficialRootRustManifest,
    installation_receipt_sha256: &'static str,
    installed_tree_sha256: &'static str,
    transform_abi: &'static str,
}

/// Whole qualified installation program, enforcing archive authority before execution.
/// Receipt publication alone cannot construct this separate source authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedRootRustInstallSource {
    source: &'static str,
    environment: BTreeMap<String, String>,
}

impl GuardedRootRustInstallSource {
    /// Complete owner program; never a precheck followed by an unguarded installer.
    #[must_use]
    pub const fn source(&self) -> &'static str {
        self.source
    }

    /// Fixed requirements, checked against the canonical cold environment.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

impl RustCompilerArtifactAuthority {
    /// Require a separately qualified whole installer for this closed cold purpose.
    /// # Errors
    /// No installer enforcing official component bytes before first execution exists.
    pub fn source_intent_install_source(
        self,
        root: SourceIntentColdRoot,
    ) -> Result<GuardedRootRustInstallSource, MiseError> {
        if self.manifest.version() != root.compiler_version()
            || self.manifest.target() != root.host().target_triple()
        {
            return Err(MiseError::Contract {
                problem: "source_intent_compiler_install_role_mismatch".to_owned(),
            });
        }
        Err(MiseError::Contract {
            problem: "source_intent_pre_first_exec_installer_unqualified: whole archive-enforcing native installation source required".to_owned(),
        })
    }

    /// Require the root role's exact Linux compiler source and installation authority.
    /// # Errors
    /// No native Linux component-to-installed-tree receipt has been qualified.
    pub fn require_root_linux() -> Result<Self, MiseError> {
        if super::RUST_VERSION != records::MANIFEST.version {
            return Err(MiseError::Contract {
                problem: "root compiler pin has no exact official manifest authority".to_owned(),
            });
        }
        Err(MiseError::Contract {
            problem: "root Linux Rust compiler artifact authority absent: official component-to-Rustup installation/full-tree and pre-first-exec enforcement qualification required".to_owned(),
        })
    }
    /// Canonical source and installation authority identity.
    #[must_use]
    pub fn qualification_digest(self) -> String {
        let mut digest = Sha256::new();
        for value in [
            "velnor-root-rust-compiler-artifact-v1",
            &self.manifest.qualification_digest(),
            self.installation_receipt_sha256,
            self.installed_tree_sha256,
            self.transform_abi,
        ] {
            digest.update(value.len().to_string().as_bytes());
            digest.update(b":");
            digest.update(value.as_bytes());
        }
        let digest = digest.finalize();
        encode_hex(&digest)
    }
    /// Fixed compiled receipt projection; external JSON cannot construct this authority.
    #[must_use]
    pub fn projection(self) -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "purpose": "root-linux-compiler-artifact-v1",
            "qualification_sha256": self.qualification_digest(),
            "manifest": self.manifest,
            "installation_receipt_sha256": self.installation_receipt_sha256,
            "installed_tree_sha256": self.installed_tree_sha256,
            "transform_abi": self.transform_abi,
        })
    }
    /// Source manifest admitted together with native installation evidence.
    #[must_use]
    pub const fn manifest(self) -> OfficialRootRustManifest {
        self.manifest
    }
    /// Exact independently qualified native installation receipt.
    #[must_use]
    pub const fn installation_receipt_sha256(self) -> &'static str {
        self.installation_receipt_sha256
    }
    /// Complete installed compiler payload, derived from official archives and transforms.
    #[must_use]
    pub const fn installed_tree_sha256(self) -> &'static str {
        self.installed_tree_sha256
    }
    /// Qualified Rustup component installation semantics.
    #[must_use]
    pub const fn transform_abi(self) -> &'static str {
        self.transform_abi
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
#[path = "catalog_rust_compiler_authority_tests.rs"]
mod tests;
