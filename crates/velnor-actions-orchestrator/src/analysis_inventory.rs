//! Complete Cargo inventory transported only through authenticated run evidence.
//!
//! Plain JSON is a payload, never authority. The remote verifier supplies the
//! capability required to construct a usable inventory; current input identity
//! is checked again before consumption. Cargo itself remains the fresh fallback.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, parse_strict_json};
use velnor_actions_rust::WorkspaceRecord;

#[path = "analysis_authority.rs"]
pub(crate) mod authority;
#[path = "analysis_inventory_inputs.rs"]
mod inputs;
#[path = "analysis_inventory_paths.rs"]
mod paths;

pub(crate) use inputs::resolution_inputs_digest;

/// Complete typed inventory payload schema; older schemas never authorize reuse.
pub(crate) const ANALYSIS_INVENTORY_SCHEMA: u32 = 1;
/// Owning Cargo source whose workspace expansion semantics are qualified.
/// Source: channel-rust-1.98.1.toml `pkg.cargo.version` (2026-09-03)
/// records `0.99.0 (797e8a9bc 2026-08-05)` for this Rust distribution.
/// Rust source 48a229cea records Cargo submodule
/// 797e8a9bca276c1c9f9f738d2a20f484fa4eea9d: this owns glob semantics.
/// https://static.rust-lang.org/dist/channel-rust-1.98.1.toml
pub(crate) const QUALIFIED_CARGO_COMMIT_PREFIX: &str = "797e8a9bc";
const QUALIFIED_CARGO_RUST_PIN: &str = "1.98.1";

/// Synthetic observed CLI identity backed by the qualified distribution source.
#[cfg(test)]
pub(crate) const QUALIFIED_CARGO_TEST_IDENTITY: &str = "cargo 1.98.1 (797e8a9bc 2026-08-05)";

/// Remote publishing run facts, verified independently of downloaded bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisSource {
    pub(crate) repository: String,
    pub(crate) head_sha: String,
    pub(crate) workflow_sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) branch: String,
}

/// Helper, observed Cargo identity, resolution inputs and immutable publisher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisIdentity {
    pub(crate) helper_sha256: String,
    pub(crate) cargo_identity: String,
    pub(crate) cargo_pin: String,
    pub(crate) resolution_inputs_digest: String,
    pub(crate) source: AnalysisSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisInventory {
    schema: u32,
    identity: AnalysisIdentity,
    inventories: Vec<(String, WorkspaceRecord)>,
}

/// Complete records validated against authenticated publication and this root.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedInventory {
    root: PathBuf,
    identity: AnalysisIdentity,
    inventories: Vec<(String, WorkspaceRecord)>,
    records: Vec<WorkspaceRecord>,
}

impl ValidatedInventory {
    /// Exact candidate entry, or a member proven by the full Cargo inventory.
    pub(crate) fn record_for(&self, manifest: &str) -> Option<WorkspaceRecord> {
        self.inventories
            .iter()
            .find(|(candidate, _)| candidate == manifest)
            .map(|(_, record)| record)
            .or_else(|| {
                self.records.iter().find(|record| {
                    record.packages.iter().any(|package| {
                        package.manifest == manifest && package.in_workspace && !package.external
                    })
                })
            })
            .cloned()
    }

    /// Exact root binding prevents proof replay into another checkout.
    pub(crate) fn applies_to(&self, root: &Path) -> bool {
        root.canonicalize().is_ok_and(|root| root == self.root)
    }

    /// Input identity must still match immediately before using the records.
    pub(crate) fn validate_current(&self, root: &Path, files: &[String]) -> Result<(), String> {
        if !self.applies_to(root) {
            return Err("analysis_inventory_root_mismatch".to_owned());
        }
        let digest = resolution_inputs_digest(root, files, &self.inventories)?;
        if digest != self.identity.resolution_inputs_digest {
            return Err("analysis_inventory_inputs_changed".to_owned());
        }
        Ok(())
    }

    /// Base graph is available only for the exact authenticated source commit.
    pub(crate) fn base_records(&self, base: &str) -> Option<&[WorkspaceRecord]> {
        (base == self.identity.source.head_sha).then_some(self.records.as_slice())
    }
}

/// Serialize every retained Cargo field, with only checkout location normalized.
pub(crate) fn build_payload(
    root: &Path,
    identity: AnalysisIdentity,
    inventories: &[(String, WorkspaceRecord)],
) -> Result<String, String> {
    validate_identity(&identity)?;
    let inventories = paths::normalize(root, inventories)?;
    let payload = AnalysisInventory {
        schema: ANALYSIS_INVENTORY_SCHEMA,
        identity,
        inventories,
    };
    let bytes = canonical_json_bytes(&payload).map_err(|err| err.to_string())?;
    String::from_utf8(bytes).map_err(|err| err.to_string())
}

/// Authenticate typed bytes against a capability constructed by the remote verifier.
pub(crate) fn parse_authenticated(
    root: &Path,
    files: &[String],
    text: &str,
    authority: &authority::RemoteAnalysisAuthority,
) -> Result<ValidatedInventory, String> {
    if !authority.authenticates(text) {
        return Err("analysis_inventory_payload_mismatch".to_owned());
    }
    let value = parse_strict_json(text).map_err(|err| err.to_string())?;
    let payload: AnalysisInventory =
        serde_json::from_value(value).map_err(|err| err.to_string())?;
    validate_identity(&payload.identity)?;
    if payload.schema != ANALYSIS_INVENTORY_SCHEMA || &payload.identity != authority.identity() {
        return Err("analysis_inventory_identity_mismatch".to_owned());
    }
    let inventories = paths::rehydrate(root, &payload.inventories)?;
    let records = velnor_actions_rust::dedupe_workspaces(
        inventories
            .iter()
            .map(|(_, record)| record.clone())
            .collect(),
    );
    let validated = ValidatedInventory {
        root: root.canonicalize().map_err(|err| err.to_string())?,
        identity: payload.identity,
        inventories,
        records,
    };
    validated.validate_current(root, files)?;
    Ok(validated)
}

/// Payload identity for the remote verifier; this does not grant authority.
pub(crate) fn payload_identity(text: &str) -> Result<AnalysisIdentity, String> {
    let value = parse_strict_json(text).map_err(|err| err.to_string())?;
    let payload: AnalysisInventory =
        serde_json::from_value(value).map_err(|err| err.to_string())?;
    if payload.schema != ANALYSIS_INVENTORY_SCHEMA {
        return Err("analysis_inventory_schema_mismatch".to_owned());
    }
    validate_identity(&payload.identity)?;
    Ok(payload.identity)
}

fn validate_identity(identity: &AnalysisIdentity) -> Result<(), String> {
    velnor_actions_contract::validate_digest(&identity.resolution_inputs_digest)
        .map_err(|err| err.to_string())?;
    if identity.helper_sha256.len() != 64
        || !identity
            .helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || identity.helper_sha256.bytes().all(|byte| byte == b'0')
        || identity.helper_sha256 == crate::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA
        || identity.cargo_pin != velnor_actions_mise::ToolCatalog::pinned().rustup_toolchain()
        || identity.cargo_pin != QUALIFIED_CARGO_RUST_PIN
        || !identity.cargo_identity.starts_with("cargo ")
        || identity.cargo_identity.lines().count() != 1
        || identity.cargo_identity.len() > 256
        || !qualified_cargo_commit(&identity.cargo_identity)
        || identity.source.run_id == 0
        || identity.source.run_attempt == 0
        || identity.source.repository.is_empty()
        || !velnor_actions_contract::is_valid_branch_name(&identity.source.branch)
    {
        return Err("analysis_inventory_invalid_identity".to_owned());
    }
    for sha in [&identity.source.head_sha, &identity.source.workflow_sha] {
        if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("analysis_inventory_invalid_source_sha".to_owned());
        }
    }
    Ok(())
}

fn qualified_cargo_commit(identity: &str) -> bool {
    identity
        == format!("cargo {QUALIFIED_CARGO_RUST_PIN} ({QUALIFIED_CARGO_COMMIT_PREFIX} 2026-08-05)")
}

#[cfg(test)]
#[path = "analysis_inventory_tests.rs"]
mod tests;
