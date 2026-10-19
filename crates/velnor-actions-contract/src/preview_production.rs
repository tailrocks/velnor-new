//! Closed leaf/union requests for unprivileged native preview production.
//!
//! Structural validation never authenticates GitHub, full history, CI success,
//! workflow authority, or artifact bytes. Runtime admission must independently
//! establish those facts and compare the request with its trusted full plan.

use serde::{Deserialize, Serialize};

use crate::{ContractError, PackageReleasePlan, canonical_json_bytes};

#[cfg(test)]
#[path = "preview_production_tests.rs"]
mod tests;
#[path = "preview_production_validation.rs"]
mod validation;

/// Closed native production request schema.
pub const PREVIEW_PRODUCTION_SCHEMA_V1: &str = "velnor.preview-production.v1";
/// Fixed complete-union job identity.
pub const PREVIEW_ASSEMBLE_JOB_ID: &str = "preview-assemble";
/// Maximum encoded request size, including its complete static plan.
pub const MAX_PREVIEW_PRODUCTION_BYTES: usize = 1024 * 1024;
/// Maximum literal leaf jobs; shared with the full release archive bound.
pub const MAX_PREVIEW_PAYLOAD_PAIRS: usize = crate::package_release::MAX_PACKAGE_RELEASE_ARCHIVES;

/// Canonically ordered product/target identity, never a reduced release plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewPayloadPair {
    /// Product identity from the complete source-bound plan.
    pub product_id: String,
    /// Exact target belonging to that product.
    pub target: String,
}

/// Numeric service locator plus exact uploaded ZIP digest for one leaf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadArtifactRef {
    /// Exact expected leaf owner.
    pub pair: PreviewPayloadPair,
    /// Positive artifact database ID, never a name-based search locator.
    pub artifact_id: u64,
    /// Mandatory lowercase SHA-256 of the service archive bytes, without prefix.
    pub sha256: String,
}

/// Claimed execution tuple. Its syntax is not remote authentication evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewProductionContext {
    /// Source repository database ID.
    pub repository_id: u64,
    /// Head repository ID, required to equal the source repository ID.
    pub head_repository_id: u64,
    /// Complete lowercase source SHA-1.
    pub source_sha: String,
    /// Exact protected default-branch ref.
    pub source_ref: String,
    /// Workflow authority commit, required to equal the source commit.
    pub workflow_authority_sha: String,
    /// Current producing preview workflow run ID.
    pub run_id: u64,
    /// Current complete producing attempt, never a failed-only reuse selector.
    pub run_attempt: u32,
    /// Upstream successful push CI run identity, independently authenticated.
    pub ci_run_id: u64,
    /// Exact upstream CI attempt.
    pub ci_run_attempt: u32,
    /// Full-history source commit count.
    pub commit_count: u64,
    /// Exact version derived from the full plan base, count and source SHA.
    pub version: String,
}

/// Closed source-task operation; neither branch carries executable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewProductionStage {
    /// Produce the complete bin inventory for exactly one declared pair.
    Payload {
        /// Selected member of the full plan's canonical pair inventory.
        pair: PreviewPayloadPair,
    },
    /// Assemble every admitted leaf with the configured opaque companions.
    Assemble {
        /// Exact sorted complete pair inventory with distinct service IDs.
        inputs: Vec<PayloadArtifactRef>,
    },
}

/// Complete static policy plus one closed operation and its execution tuple.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewProductionRequest {
    /// Exact versioned request schema.
    pub schema: String,
    /// Full unchanged static plan, including every product and companion.
    pub plan: PackageReleasePlan,
    /// SHA-256 of the complete validated canonical static plan bytes.
    pub plan_sha256: String,
    /// Claimed execution identity, requiring independent runtime admission.
    pub context: PreviewProductionContext,
    /// One leaf or the complete exact assembly operation.
    pub stage: PreviewProductionStage,
}

impl PreviewProductionRequest {
    /// Check bounds, internal consistency and the closed operation inventory.
    /// # Errors
    /// Rejects invalid plans, contexts, digests, pairs and assembly inventories.
    pub fn validate(&self) -> Result<(), ContractError> {
        validation::validate(self)
    }

    /// Compare against independently admitted full policy and execution inputs.
    /// This pure comparison does not establish how the caller obtained them.
    /// # Errors
    /// Rejects any structurally invalid request or policy/context substitution.
    pub fn validate_against(
        &self,
        trusted_plan: &PackageReleasePlan,
        authenticated_context: &PreviewProductionContext,
    ) -> Result<(), ContractError> {
        self.validate()?;
        if &self.plan != trusted_plan || &self.context != authenticated_context {
            return Err(invalid("trusted_plan_or_context_mismatch"));
        }
        Ok(())
    }

    /// Encode bounded canonical request bytes after structural validation.
    /// # Errors
    /// Rejects invalid or oversized requests and serialization errors.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self)?;
        crate::strict_json::check_document_size(bytes.len(), MAX_PREVIEW_PRODUCTION_BYTES)?;
        Ok(bytes)
    }
}

/// Derive the sole sorted literal leaf inventory from a validated full plan.
/// # Errors
/// Rejects non-preview, invalid, empty or oversized product/target inventories.
pub fn expected_preview_payload_pairs(
    plan: &PackageReleasePlan,
) -> Result<Vec<PreviewPayloadPair>, ContractError> {
    plan.validate()?;
    if plan.channel != crate::package_release::PackageReleaseChannel::Preview {
        return Err(invalid("preview_plan_required"));
    }
    let pairs = plan
        .products
        .iter()
        .flat_map(|product| {
            product.targets.iter().map(|target| PreviewPayloadPair {
                product_id: product.id.clone(),
                target: target.clone(),
            })
        })
        .collect::<Vec<_>>();
    if pairs.is_empty() || pairs.len() > MAX_PREVIEW_PAYLOAD_PAIRS {
        return Err(invalid("payload_pair_count"));
    }
    Ok(pairs)
}

/// Compute SHA-256 from validated canonical full-plan bytes, never a leaf plan.
/// # Errors
/// Rejects invalid full plans or canonical encoding failures.
pub fn preview_plan_sha256(plan: &PackageReleasePlan) -> Result<String, ContractError> {
    use sha2::{Digest as _, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(plan.canonical_bytes()?);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

/// Derive the fixed machine job ID from the pair's canonical full-plan index.
/// # Errors
/// Rejects invalid full plans and pairs outside their bounded inventory.
pub fn preview_payload_job_id(
    plan: &PackageReleasePlan,
    pair: &PreviewPayloadPair,
) -> Result<String, ContractError> {
    let pairs = expected_preview_payload_pairs(plan)?;
    let index = pairs
        .binary_search(pair)
        .map_err(|_| invalid("unknown_payload_pair"))?;
    Ok(format!("preview-payload-{index:03}"))
}

/// Validate the claimed context against the complete preview plan's syntax.
/// This does not authenticate GitHub, source history, CI success or currentness.
/// # Errors
/// Rejects non-preview/invalid full plans and inconsistent execution identities.
pub fn validate_preview_production_context(
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
) -> Result<(), ContractError> {
    expected_preview_payload_pairs(plan)?;
    validation::validate_context(context, plan)
}

/// Derive the fixed aggregate locator, never artifact acquisition authority.
/// # Errors
/// Rejects invalid full plans and inconsistent execution identities.
pub fn preview_aggregate_artifact_name(
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
) -> Result<String, ContractError> {
    validate_preview_production_context(plan, context)?;
    Ok(format!(
        "velnor-preview-aggregate-r{}-a{}",
        context.run_id, context.run_attempt
    ))
}

/// Derive a deterministic leaf locator, never download or authentication authority.
/// # Errors
/// Rejects invalid context, full plan or a pair outside that plan.
pub fn preview_payload_artifact_name(
    plan: &PackageReleasePlan,
    pair: &PreviewPayloadPair,
    context: &PreviewProductionContext,
) -> Result<String, ContractError> {
    let pairs = expected_preview_payload_pairs(plan)?;
    validation::validate_context(context, plan)?;
    if pairs.binary_search(pair).is_err() {
        return Err(invalid("unknown_payload_pair"));
    }
    Ok(format!(
        "velnor-preview-payload-r{}-a{}-{}-{}",
        context.run_id, context.run_attempt, pair.product_id, pair.target
    ))
}

/// Parse strict, bounded, byte-for-byte canonical native production JSON.
/// # Errors
/// Rejects duplicate/unknown fields, invalid UTF-8, noncanonical bytes or values.
pub fn parse_preview_production_request(
    bytes: &[u8],
) -> Result<PreviewProductionRequest, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, MAX_PREVIEW_PRODUCTION_BYTES)?;
    let request: PreviewProductionRequest = serde_json::from_value(value)
        .map_err(|error| ContractError::identity("preview_production", error.to_string()))?;
    if request.canonical_bytes()?.as_slice() != bytes {
        return Err(invalid("noncanonical_json"));
    }
    Ok(request)
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("preview_production", problem)
}
