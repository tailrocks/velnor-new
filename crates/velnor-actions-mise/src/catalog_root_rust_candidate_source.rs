//! Reviewed compiler candidate inputs. This type grants no installed execution rights.

use super::OfficialRootRustManifest;
use crate::MiseError;
use crate::catalog::rust_desktop::RustCompilerRole;
use crate::catalog::rustup_authority::RustupManagerAuthority;
use serde_json::{Value, json};

const PAYLOAD: &str = include_str!("catalog_root_rust_candidate_payload.json");
const PAYLOAD_SHA256: &str = "55e828f3ae021b440f1fac6df570295a40f5411b6a527ffa771cdaebb71ca1f4";
const TRANSFORM_ABI: &str = "rustup-native-file-components-source-v1";

/// Opaque source inputs for the first `RootLinux` candidate qualification run.
/// No deserialization, caller paths, receipt data, or observed self-hash mints this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRustCandidateSource {
    manifest: OfficialRootRustManifest,
    payload: Value,
}

impl RootRustCandidateSource {
    /// Select the reviewed official archives and their native Rustup transform map.
    /// This does not require or grant a production installed compiler receipt.
    /// # Errors
    /// Rejects inconsistent compiled source inputs or a changed selected root version.
    pub fn require_root_linux() -> Result<Self, MiseError> {
        let manifest = OfficialRootRustManifest::audit();
        let manager = RustupManagerAuthority::for_role(RustCompilerRole::RootLinux);
        if manifest.version() != manager.compiler_version()
            || manifest.target() != manager.host().target_triple()
            || sha256(PAYLOAD.as_bytes()) != PAYLOAD_SHA256
        {
            return Err(invalid(
                "compiled candidate source inputs differ from reviewed identity",
            ));
        }
        let payload: Value = serde_json::from_str(PAYLOAD)
            .map_err(|_| invalid("compiled candidate payload is malformed"))?;
        if payload.as_array().map(Vec::len) != Some(156) {
            return Err(invalid(
                "compiled candidate payload does not contain all 156 files",
            ));
        }
        Ok(Self { manifest, payload })
    }

    /// Existing sole official manifest/component authority.
    #[must_use]
    pub const fn manifest(&self) -> OfficialRootRustManifest {
        self.manifest
    }

    /// Complete selected component files, mapped before the first compiler launch.
    #[must_use]
    pub fn payload(&self) -> Value {
        self.payload.clone()
    }

    /// Reviewed native Rustup file extraction, mode normalization, and rename behavior.
    #[must_use]
    pub const fn transform_abi(&self) -> &'static str {
        TRANSFORM_ABI
    }

    /// Digest of every source input and fixed transform constraint, without runtime grant.
    #[must_use]
    pub fn qualification_digest(&self) -> String {
        sha256(self.identity_projection().to_string().as_bytes())
    }

    /// Source-only compiled data for a closed candidate installer program.
    /// Host receipts and Foundation admission remain separate runtime prerequisites.
    #[must_use]
    pub fn projection(&self) -> Value {
        let mut value = self.identity_projection();
        value["qualification_sha256"] = json!(self.qualification_digest());
        value
    }

    fn identity_projection(&self) -> Value {
        let manager = RustupManagerAuthority::for_role(RustCompilerRole::RootLinux);
        let source = manager.qualification_descriptor();
        json!({
            "schema": 1,
            "purpose": "root-linux-candidate-artifact-v1",
            "authority": "source-inputs-only",
            "manifest": self.manifest,
            "manifest_identity_sha256": self.manifest.qualification_digest(),
            "payload": self.payload,
            "payload_sha256": PAYLOAD_SHA256,
            "manager": {
                "version": manager.version(),
                "url": manager.archive_url(),
                "sha256": manager.sha256(),
                "source_repository": "https://github.com/rust-lang/rustup",
                "source_commit": source.source_commit(),
                "source_tree": source.source_tree(),
            },
            "transform_abi": TRANSFORM_ABI,
            "constraints": {
                "transport": "verified-local-file-dist-v2-only",
                "profile": "minimal",
                "components": ["clippy", "rustfmt"],
                "umask": "022",
                "fresh_exclusive_roots": true,
                "same_filesystem_temporary_and_toolchain": true,
                "permit_copy_rename": "absent",
                "automatic_install": false,
                "self_update": false,
                "force": false,
                "ambient_compiler_path": false,
                "legacy_manifest_fallback": false,
                "foundation_execution_admission": "independent-required",
            },
        })
    }
}

fn sha256(bytes: &[u8]) -> String {
    velnor_actions_contract::compiled_source_sha256(bytes)
}

fn invalid(message: &str) -> MiseError {
    MiseError::Contract {
        problem: format!("root compiler candidate source: {message}"),
    }
}

#[cfg(test)]
#[path = "catalog_root_rust_candidate_source_tests.rs"]
mod tests;
