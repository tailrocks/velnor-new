//! Public source bytes depend on source evidence, independently of the compiler.

use serde::Serialize;
use velnor_actions_contract::digest_b3;

use super::{MAX_JSON_BYTES, RustSourceDescriptor, RustSourceSelection, contract};
use crate::OrchestratorError;

/// Describes admitted source evidence; it grants no execution or reuse authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustSourceProjection {
    canonical_bytes: Vec<u8>,
    digest: String,
}

impl RustSourceProjection {
    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Schema-1 source projection digest, independent of helper program identity.
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

pub(super) fn projection(
    descriptor: &RustSourceDescriptor,
) -> Result<RustSourceProjection, OrchestratorError> {
    let canonical_bytes = json(descriptor)?;
    let mut evidence = b"velnor-rust-source-projection-v1\0".to_vec();
    evidence.extend_from_slice(&canonical_bytes);
    Ok(RustSourceProjection {
        canonical_bytes,
        digest: digest_b3(&evidence),
    })
}

#[derive(Serialize)]
struct SourceIdentity<'a> {
    schema: u32,
    target: &'a str,
    roots: &'a [String],
    manifests: &'a [(String, String)],
    locks: &'a [(String, String)],
    archives: &'a [(String, String, String)],
    mode: &'a str,
    #[serde(skip_serializing_if = "<[RustSourceSelection]>::is_empty")]
    selections: &'a [RustSourceSelection],
}

pub(super) fn json(descriptor: &RustSourceDescriptor) -> Result<Vec<u8>, OrchestratorError> {
    // Exhaustive matching requires each new descriptor field to receive an
    // explicit identity decision. Compiler evidence remains in the runtime
    // descriptor for producer verification and receipts.
    let RustSourceDescriptor {
        schema,
        rust_version: _,
        target,
        roots,
        manifests,
        locks,
        archives,
        mode,
        selections,
    } = descriptor;
    let identity = SourceIdentity {
        schema: *schema,
        target,
        roots,
        manifests,
        locks,
        archives,
        mode,
        selections,
    };
    let bytes = serde_json::to_vec(&identity).map_err(contract)?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(contract("rust_source_descriptor_too_large"));
    }
    Ok(bytes)
}
