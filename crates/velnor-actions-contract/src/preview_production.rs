use serde::{Deserialize, Serialize};

use crate::{ContractError, PackageReleasePlan, canonical_json_bytes};

#[path = "preview_production_validation.rs"]
mod validation;

pub const RELEASE_PRODUCTION_SCHEMA_V1: &str = "velnor.release-production.v1";

pub const MAX_RELEASE_PRODUCTION_BYTES: usize = 1024 * 1024;

pub const MAX_RELEASE_PAYLOAD_PAIRS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePayloadPair {
    pub product_id: String,

    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadArtifactRef {
    pub pair: ReleasePayloadPair,

    pub artifact_id: u64,

    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseProductionContext {
    pub repository_id: u64,

    pub head_repository_id: u64,

    pub source_sha: String,

    pub source_ref: String,

    pub workflow_authority_sha: String,

    pub run_id: u64,

    pub run_attempt: u32,

    pub ci_run_id: u64,

    pub ci_run_attempt: u32,

    pub commit_count: u64,

    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReleaseProductionStage {
    Payload { pair: ReleasePayloadPair },

    Assemble { inputs: Vec<PayloadArtifactRef> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseProductionRequest {
    pub schema: String,

    pub plan: PackageReleasePlan,

    pub policy_sha256: String,

    pub plan_sha256: String,

    pub context: ReleaseProductionContext,

    pub stage: ReleaseProductionStage,
}

impl ReleaseProductionRequest {
    pub fn validate(&self) -> Result<(), ContractError> {
        validation::validate(self)
    }

    pub fn validate_against(
        &self,
        trusted_policy: &crate::PackageReleasePolicy,
        trusted_plan: &PackageReleasePlan,
        authenticated_context: &ReleaseProductionContext,
    ) -> Result<(), ContractError> {
        self.validate()?;
        trusted_policy.validate_plan(trusted_plan)?;
        if self.policy_sha256 != crate::canonical::sha256_hex(&trusted_policy.canonical_bytes()?)
            || &self.plan != trusted_plan
            || &self.context != authenticated_context
        {
            return Err(invalid("trusted_plan_or_context_mismatch"));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self)?;
        crate::strict_json::check_document_size(bytes.len(), MAX_RELEASE_PRODUCTION_BYTES)?;
        Ok(bytes)
    }
}

pub fn expected_release_payload_pairs(
    plan: &PackageReleasePlan,
) -> Result<Vec<ReleasePayloadPair>, ContractError> {
    plan.validate()?;
    let pairs = plan
        .products
        .iter()
        .flat_map(|product| {
            product.targets.iter().map(|target| ReleasePayloadPair {
                product_id: product.id.clone(),
                target: target.clone(),
            })
        })
        .collect::<Vec<_>>();
    if pairs.is_empty() || pairs.len() > MAX_RELEASE_PAYLOAD_PAIRS {
        return Err(invalid("payload_pair_count"));
    }
    Ok(pairs)
}

pub fn release_plan_sha256(plan: &PackageReleasePlan) -> Result<String, ContractError> {
    Ok(crate::canonical::sha256_hex(&plan.canonical_bytes()?))
}

pub fn release_payload_job_id(
    plan: &PackageReleasePlan,
    pair: &ReleasePayloadPair,
) -> Result<String, ContractError> {
    let pairs = expected_release_payload_pairs(plan)?;
    let index = pairs
        .binary_search(pair)
        .map_err(|_| invalid("unknown_payload_pair"))?;
    match plan.channel {
        crate::package_release::PackageReleaseChannel::Preview => Ok("preview-payload".to_owned()),
        crate::package_release::PackageReleaseChannel::Stable => {
            Ok(format!("stable-payload-{index:03}"))
        }
    }
}

pub fn validate_release_production_context(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
) -> Result<(), ContractError> {
    expected_release_payload_pairs(plan)?;
    validation::validate_context(context, plan)
}

pub fn release_aggregate_artifact_name(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
) -> Result<String, ContractError> {
    validate_release_production_context(plan, context)?;
    Ok(format!(
        "velnor-release-aggregate-r{}-a{}",
        context.run_id, context.run_attempt
    ))
}

pub fn release_payload_artifact_name(
    plan: &PackageReleasePlan,
    pair: &ReleasePayloadPair,
    context: &ReleaseProductionContext,
) -> Result<String, ContractError> {
    let pairs = expected_release_payload_pairs(plan)?;
    validation::validate_context(context, plan)?;
    if pairs.binary_search(pair).is_err() {
        return Err(invalid("unknown_payload_pair"));
    }
    Ok(format!(
        "velnor-release-payload-r{}-a{}-{}-{}",
        context.run_id, context.run_attempt, pair.product_id, pair.target
    ))
}

pub fn parse_release_production_request(
    bytes: &[u8],
) -> Result<ReleaseProductionRequest, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, MAX_RELEASE_PRODUCTION_BYTES)?;
    let request: ReleaseProductionRequest = serde_json::from_value(value)
        .map_err(|error| ContractError::identity("release_production", error.to_string()))?;
    if request.canonical_bytes()?.as_slice() != bytes {
        return Err(invalid("noncanonical_json"));
    }
    Ok(request)
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("release_production", problem)
}
