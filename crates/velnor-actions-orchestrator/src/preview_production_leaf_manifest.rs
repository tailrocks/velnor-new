//! Canonical native leaf and full-union evidence; neither is a public receipt.

use super::{
    PreviewProducerProof,
    handoff::{AssetProof, MAX_NATIVE_MANIFEST, validate_asset_name},
};
use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    canonical_json_bytes,
    config::RustBinaryReleaseConfig,
    preview_production::{ReleaseProductionRequest, ReleaseProductionStage},
};

pub(crate) const LEAF_SCHEMA: &str = "velnor.release-payload.v1";
pub(crate) const AGGREGATE_SCHEMA: &str = "velnor.release-aggregate.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewLeafManifest {
    pub schema: String,
    pub request: ReleaseProductionRequest,
    pub producer: PreviewProducerProof,
    pub assets: Vec<AssetProof>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewAggregateManifest {
    pub schema: String,
    pub request: ReleaseProductionRequest,
    pub producer: PreviewProducerProof,
    pub leaves: Vec<PreviewLeafManifest>,
    pub assets: Vec<AssetProof>,
}

impl PreviewLeafManifest {
    pub(crate) fn parse(
        bytes: &[u8],
        expected: &ReleaseProductionRequest,
    ) -> Result<Self, OrchestratorError> {
        let manifest: Self = parse(bytes)?;
        if manifest.schema != LEAF_SCHEMA || manifest.request != *expected {
            return Err(internal("preview_leaf_request_mismatch"));
        }
        manifest
            .request
            .validate()
            .map_err(|_| internal("preview_leaf_request"))?;
        let ReleaseProductionStage::Payload { pair } = &manifest.request.stage else {
            return Err(internal("preview_leaf_stage"));
        };
        let archive = format!("{}-{}.tar.gz", pair.product_id, pair.target);
        let mut names = [archive.clone(), format!("{archive}.sbom.json")];
        names.sort();
        if manifest
            .assets
            .iter()
            .map(|asset| &asset.name)
            .collect::<Vec<_>>()
            != names.iter().collect::<Vec<_>>()
        {
            return Err(internal("preview_leaf_asset_inventory"));
        }
        validate_assets(&manifest.assets)?;
        if manifest.assets.iter().any(|asset| asset.size == 0) {
            return Err(internal("preview_leaf_empty_asset"));
        }
        validate_producer(&manifest.producer, &manifest.request)?;
        Ok(manifest)
    }

    pub(super) fn validate_source(
        &self,
        preview: &RustBinaryReleaseConfig,
    ) -> Result<(), OrchestratorError> {
        if self.producer.task != preview.producer.task
            || self.producer.output_directory != preview.producer.output_directory
            || self.producer.output_manifest != preview.producer.manifest
        {
            return Err(internal("preview_leaf_producer_policy"));
        }
        Ok(())
    }
}

impl PreviewAggregateManifest {
    pub(crate) fn validate_source(
        &self,
        preview: &RustBinaryReleaseConfig,
    ) -> Result<(), OrchestratorError> {
        for leaf in &self.leaves {
            leaf.validate_source(preview)?;
        }
        if self.producer.task != preview.producer.task
            || self.producer.output_directory != preview.producer.output_directory
            || self.producer.output_manifest != preview.producer.manifest
        {
            return Err(internal("preview_aggregate_producer_policy"));
        }
        Ok(())
    }
    pub(crate) fn parse(
        bytes: &[u8],
        policy_sha256: &str,
        plan: &velnor_actions_contract::PackageReleasePlan,
        context: &velnor_actions_contract::preview_production::ReleaseProductionContext,
    ) -> Result<Self, OrchestratorError> {
        let manifest: Self = parse(bytes)?;
        manifest
            .request
            .validate()
            .map_err(|_| internal("preview_aggregate_request_mismatch"))?;
        if manifest.request.policy_sha256 != policy_sha256
            || &manifest.request.plan != plan
            || &manifest.request.context != context
        {
            return Err(internal("release_aggregate_policy_plan_context_mismatch"));
        }
        let ReleaseProductionStage::Assemble { inputs } = &manifest.request.stage else {
            return Err(internal("preview_aggregate_stage"));
        };
        if manifest.schema != AGGREGATE_SCHEMA || manifest.leaves.len() != inputs.len() {
            return Err(internal("preview_aggregate_inventory"));
        }
        validate_assets(&manifest.assets)?;
        validate_producer(&manifest.producer, &manifest.request)?;
        let mut expected_names = plan
            .companions
            .iter()
            .map(|row| row.asset_name.clone())
            .collect::<Vec<_>>();
        for (leaf, input) in manifest.leaves.iter().zip(inputs) {
            let mut expected = manifest.request.clone();
            expected.stage = ReleaseProductionStage::Payload {
                pair: input.pair.clone(),
            };
            PreviewLeafManifest::parse(&encode(leaf)?, &expected)?;
            for asset in &leaf.assets {
                if !manifest.assets.contains(asset) {
                    return Err(internal("preview_aggregate_leaf_changed"));
                }
                expected_names.push(asset.name.clone());
            }
        }
        expected_names.sort();
        if manifest
            .assets
            .iter()
            .map(|row| &row.name)
            .collect::<Vec<_>>()
            != expected_names.iter().collect::<Vec<_>>()
        {
            return Err(internal("preview_aggregate_inventory"));
        }
        Ok(manifest)
    }
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, OrchestratorError> {
    let bytes =
        canonical_json_bytes(value).map_err(|_| internal("preview_native_manifest_encode"))?;
    if bytes.len() as u64 > MAX_NATIVE_MANIFEST {
        return Err(internal("preview_native_manifest_size"));
    }
    Ok(bytes)
}

fn parse<T: serde::de::DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T, OrchestratorError> {
    let limit = usize::try_from(MAX_NATIVE_MANIFEST)
        .map_err(|_| internal("preview_native_manifest_size"))?;
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(bytes, limit)
        .map_err(|_| internal("preview_native_manifest_json"))?;
    let parsed: T =
        serde_json::from_value(value).map_err(|_| internal("preview_native_manifest_shape"))?;
    if encode(&parsed)? != bytes {
        return Err(internal("preview_native_manifest_noncanonical"));
    }
    Ok(parsed)
}

pub(super) fn validate_assets(assets: &[AssetProof]) -> Result<(), OrchestratorError> {
    if assets.is_empty() || assets.windows(2).any(|rows| rows[0].name >= rows[1].name) {
        return Err(internal("preview_native_manifest_inventory"));
    }
    for asset in assets {
        validate_asset_name(&asset.name)?;
        if asset.sha256.len() != 64
            || !asset
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(internal("preview_native_manifest_asset"));
        }
    }
    Ok(())
}

fn validate_producer(
    producer: &PreviewProducerProof,
    request: &ReleaseProductionRequest,
) -> Result<(), OrchestratorError> {
    let context = &request.context;
    if producer.source_sha != context.source_sha
        || producer.source_ref != context.source_ref
        || producer.version != context.version
        || producer.commit_count != context.commit_count
        || producer.task_exit_code != 0
        || producer.duration_ms == 0
        || producer.qualification_digest.is_empty()
    {
        return Err(internal("preview_native_producer_identity"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "preview_production_leaf_manifest_tests.rs"]
mod tests;
