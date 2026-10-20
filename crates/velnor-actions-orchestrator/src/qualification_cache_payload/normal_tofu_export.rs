//! Isolated one-file export for the normal `OpenTofu` provider cache.

use super::super::manifest::PayloadManifest;
use super::super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::super::transfer::{snapshot_roots, useful_state_digest, write_transport_payload};
use super::{NormalTofuProvidersCachePayloadRequest, validate_request};
use std::path::{Path, PathBuf};
use velnor_actions_contract::QualificationCacheLayer;

/// Evidence that provider bytes were encoded into the isolated action stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalTofuProvidersCacheExport {
    state_digest: String,
    manifest_digest: String,
    cache_key: String,
    stage_path: PathBuf,
    stage_claim: String,
}

impl NormalTofuProvidersCacheExport {
    /// Gives the useful-state digest written into the manifest.
    #[must_use]
    pub fn state_digest(&self) -> &str {
        &self.state_digest
    }

    /// Gives the manifest digest for this staged export.
    #[must_use]
    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    /// Gives the exact cache key bound into the manifest.
    #[must_use]
    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    /// Gives the one-file path passed to the cache action.
    #[must_use]
    pub fn stage_path(&self) -> &Path {
        &self.stage_path
    }

    /// Gives the untrusted compact file-identity claim for workflow output.
    #[must_use]
    pub fn stage_claim(&self) -> &str {
        &self.stage_claim
    }
}

/// Encode the current provider-cache root into the checked one-file transport.
///
/// This records local staging only. It does not say that the cache service
/// stored the bytes.
///
/// # Errors
///
/// Gives a static error code when runtime facts, roots, stage custody, or the
/// encoded payload fail validation.
pub fn export_normal_tofu_providers_cache_payload(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
) -> Result<NormalTofuProvidersCacheExport, &'static str> {
    let runtime = validate_request(request)?;
    let roots = CacheRootSet {
        owner_id: "normal-tofu-providers".to_owned(),
        layer: QualificationCacheLayer::TofuProviders,
        roots: vec![CacheRootObservation {
            id: CacheRootId::TofuPlugins,
            path: runtime.cache_path.clone(),
        }],
    };
    let payload_roots = snapshot_roots(&roots)?;
    let state_digest = useful_state_digest(&payload_roots)?;
    let manifest = PayloadManifest::for_normal_tofu_save(
        request.cache_key,
        &runtime,
        &state_digest,
        &roots,
        payload_roots,
    )?;
    let manifest_text = manifest.canonical_json()?;
    let manifest_digest = format!("b3-{}", blake3::hash(manifest_text.as_bytes()).to_hex());
    let stage_context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        request.runner_temp,
        request.workspace,
        request.cache_key,
    )
    .map_err(|_| "normal_tofu_stage_context_invalid")?;
    let stage_path = stage_context.action_path().to_owned();
    let mut writer = match request.previous_stage_claim {
        Some(text) => {
            let claim = crate::cache_action_stage::StageReceiptClaim::parse(text)
                .map_err(|_| "normal_tofu_stage_claim_invalid")?;
            crate::cache_action_stage::rewrite_stage(&stage_context, &claim)
                .map_err(|_| "normal_tofu_stage_rewrite_failed")?
        }
        None => crate::cache_action_stage::create_stage(&stage_context)
            .map_err(|_| "normal_tofu_stage_create_failed")?,
    };
    write_transport_payload(
        &mut writer,
        request.operation_limit_bytes,
        &roots,
        &manifest.roots,
        &manifest_text,
    )?;
    let stage_claim = writer
        .finish()
        .map_err(|_| "normal_tofu_stage_finish_failed")?
        .encode();
    let claim = crate::cache_action_stage::StageReceiptClaim::parse(&stage_claim)
        .map_err(|_| "normal_tofu_stage_claim_invalid")?;
    let mut reader = crate::cache_action_stage::open_stage(&stage_context, &claim)
        .map_err(|_| "normal_tofu_stage_verify_failed")?;
    super::super::transfer::validate_transport_envelope(
        &mut reader,
        request.operation_limit_bytes,
    )?;
    reader
        .revalidate()
        .map_err(|_| "normal_tofu_stage_verify_failed")?;
    Ok(NormalTofuProvidersCacheExport {
        state_digest,
        manifest_digest,
        cache_key: request.cache_key.to_owned(),
        stage_path,
        stage_claim,
    })
}

#[cfg(test)]
#[path = "tests/normal_tofu_source_authority_tests.rs"]
mod source_authority_tests;
