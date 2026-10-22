//! Backend evidence for normal `OpenTofu` provider-cache restores.

use super::super::NormalTofuProvidersCachePayloadRequest;
use super::super::ValidatedTofuRuntime;
use super::context::{GitHubRunContext, read_github_run_context};
use crate::cache_action_stage::{CacheActionCompression, StageContext, StageReceiptClaim};
use crate::qualification_cache_payload::CacheActionObservation;
use crate::qualification_cache_payload::roots::CacheRootSet;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
};

const MAX_CACHE_TRANSFER_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const CACHE_ACTION_TAR_OVERHEAD_BYTES: u64 = 32 * 1024;
const CACHE_ACTION_COMPRESS_HEADROOM_BYTES: u64 = 1024 * 1024;
const CACHE_ACTION_COMPRESS_HEADROOM_DENOMINATOR: u64 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Backend agreement for one normal `ToFu` restore, before payload checks.
pub struct NormalTofuProvidersCacheBackendEvidence {
    run: GitHubRunContext,
    target: String,
    tofu_version: String,
    root: String,
    lockfile_sha256: String,
    cache_key: String,
    action_hit: bool,
    action_matched_key: Option<String>,
    cache: Option<QualificationCacheBackendEntry>,
    compression: Option<CacheActionCompression>,
    stage_claim: Option<StageReceiptClaim>,
    payload_state_digest: Option<String>,
}

impl NormalTofuProvidersCacheBackendEvidence {
    /// Gives the measured native target.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Gives the measured `OpenTofu` version.
    #[must_use]
    pub fn tofu_version(&self) -> &str {
        &self.tofu_version
    }

    /// Gives the normalized provider root.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Gives the measured lockfile SHA-256.
    #[must_use]
    pub fn lockfile_sha256(&self) -> &str {
        &self.lockfile_sha256
    }

    /// Gives the exact full key checked by the census.
    #[must_use]
    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    /// Gives the observed repository slug.
    #[must_use]
    pub fn repository(&self) -> &str {
        &self.run.repository
    }

    /// Gives the numeric repository ID checked against the event document.
    #[must_use]
    pub const fn repository_id(&self) -> u64 {
        self.run.repository_id
    }

    /// Gives the exact runner ref used for the census.
    #[must_use]
    pub fn git_ref(&self) -> &str {
        &self.run.git_ref
    }

    /// Gives the source SHA from the current runner context.
    #[must_use]
    pub fn source_sha(&self) -> &str {
        &self.run.source_sha
    }

    /// Gives the workflow ref from the current runner context.
    #[must_use]
    pub fn workflow_ref(&self) -> &str {
        &self.run.workflow_ref
    }

    /// Gives the workflow SHA from the current runner context.
    #[must_use]
    pub fn workflow_sha(&self) -> &str {
        &self.run.workflow_sha
    }

    /// Gives the current workflow run ID.
    #[must_use]
    pub const fn run_id(&self) -> u64 {
        self.run.run_id
    }

    /// Gives the current workflow attempt.
    #[must_use]
    pub const fn run_attempt(&self) -> u32 {
        self.run.run_attempt
    }

    /// Gives the cache restore action's hit result.
    #[must_use]
    pub const fn action_hit(&self) -> bool {
        self.action_hit
    }

    /// Gives the key reported by the cache restore action.
    #[must_use]
    pub fn action_matched_key(&self) -> Option<&str> {
        self.action_matched_key.as_deref()
    }

    /// Gives the unique exact backend entry, when the action reports a hit.
    #[must_use]
    pub fn cache_entry(&self) -> Option<&QualificationCacheBackendEntry> {
        self.cache.as_ref()
    }

    /// Gives the compression method observed by the cache tar adapter.
    #[must_use]
    pub const fn compression(&self) -> Option<CacheActionCompression> {
        self.compression
    }

    /// Gives the untrusted stage claim from the cache tar adapter.
    #[must_use]
    pub fn stage_claim(&self) -> Option<&StageReceiptClaim> {
        self.stage_claim.as_ref()
    }

    /// Gives the validated manifest state digest when a cache hit payload passed preflight.
    #[must_use]
    pub fn payload_state_digest(&self) -> Option<&str> {
        self.payload_state_digest.as_deref()
    }
}

/// Check the action output and exact cache entry, then preflight a cache-hit payload.
///
/// The returned evidence includes the useful-state digest only after the
/// bounded transport, manifest, inventory, and member digests all validate.
/// This does not install provider bytes into the live cache root.
///
/// # Errors
///
/// Gives a static error code when runner context, action output, or backend
/// evidence does not agree.
pub fn observe_normal_tofu_providers_cache_backend(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    action: &CacheActionObservation,
    stage_claim: Option<&str>,
    compression: Option<CacheActionCompression>,
) -> Result<NormalTofuProvidersCacheBackendEvidence, &'static str> {
    let runtime = super::super::validate_request(request)?;
    let run = read_github_run_context()?;
    let observed = crate::qualification_cache_backend::observe_qualification_cache_backend(
        request.workspace,
        &run.repository,
        request.cache_key,
        &run.git_ref,
    )
    .map_err(|_| "normal_tofu_cache_backend_unavailable")?;
    let mut evidence = validate_restore_observation(
        request,
        action,
        stage_claim,
        compression,
        run,
        runtime.clone(),
        observed,
    )?;
    if let Some(claim) = evidence.stage_claim.as_ref() {
        evidence.payload_state_digest = Some(validate_cache_hit_payload(request, &runtime, claim)?);
    }
    Ok(evidence)
}

fn validate_cache_hit_payload(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    runtime: &ValidatedTofuRuntime,
    claim: &StageReceiptClaim,
) -> Result<String, &'static str> {
    let context = StageContext::normal_tofu_providers(
        request.runner_temp,
        request.workspace,
        request.cache_key,
    )
    .map_err(|_| "normal_tofu_stage_context_invalid")?;
    let mut reader = crate::cache_action_stage::open_stage(&context, claim)
        .map_err(|_| "normal_tofu_cache_stage_invalid")?;
    let roots = CacheRootSet {
        owner_id: "normal-tofu-providers".to_owned(),
        layer: velnor_actions_contract::QualificationCacheLayer::TofuProviders,
        roots: vec![super::super::super::roots::CacheRootObservation {
            id: super::super::super::roots::CacheRootId::TofuPlugins,
            path: runtime.cache_path.clone(),
        }],
    };
    let transport = super::super::super::transfer::transport_tar_read::preflight(
        &mut reader,
        request.operation_limit_bytes,
        &roots,
        |text| {
            super::super::super::manifest::PayloadManifest::parse_for_normal_tofu_restore(
                text,
                request.cache_key,
                runtime,
                &roots,
            )
        },
    )?;
    reader
        .revalidate()
        .map_err(|_| "normal_tofu_cache_stage_invalid")?;
    Ok(transport.state_digest)
}

pub(super) fn validate_restore_observation(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    action: &CacheActionObservation,
    stage_claim: Option<&str>,
    compression: Option<CacheActionCompression>,
    run: GitHubRunContext,
    runtime: super::super::ValidatedTofuRuntime,
    observed: QualificationCacheBackendObservation,
) -> Result<NormalTofuProvidersCacheBackendEvidence, &'static str> {
    validate_action_key(request.cache_key, action)?;
    if !action.hit {
        return validate_cache_miss(
            request,
            action,
            stage_claim,
            compression,
            run,
            runtime,
            &observed,
        );
    }

    let stage_claim = stage_claim.ok_or("normal_tofu_cache_stage_claim_missing")?;
    let stage_claim = StageReceiptClaim::parse(stage_claim)
        .map_err(|_| "normal_tofu_cache_stage_claim_invalid")?;
    let compression = compression.ok_or("normal_tofu_cache_compression_missing")?;
    validate_cache_hit(
        request,
        action,
        run,
        runtime,
        stage_claim,
        compression,
        observed,
    )
}

fn validate_action_key(
    cache_key: &str,
    action: &CacheActionObservation,
) -> Result<(), &'static str> {
    if action.hit != action.matched_key.is_some()
        || action
            .matched_key
            .as_deref()
            .is_some_and(|key| key != cache_key)
    {
        return Err("normal_tofu_cache_action_mismatch");
    }
    Ok(())
}

fn validate_cache_miss(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    action: &CacheActionObservation,
    stage_claim: Option<&str>,
    compression: Option<CacheActionCompression>,
    run: GitHubRunContext,
    runtime: super::super::ValidatedTofuRuntime,
    observed: &QualificationCacheBackendObservation,
) -> Result<NormalTofuProvidersCacheBackendEvidence, &'static str> {
    if stage_claim.is_some() || compression.is_some() {
        return Err("normal_tofu_cache_miss_has_stage_evidence");
    }
    match observed {
        QualificationCacheBackendObservation::Absent => {
            Ok(NormalTofuProvidersCacheBackendEvidence {
                run,
                target: runtime.target,
                tofu_version: runtime.tofu_version,
                root: runtime.root,
                lockfile_sha256: runtime.lockfile_sha256,
                cache_key: request.cache_key.to_owned(),
                action_hit: action.hit,
                action_matched_key: action.matched_key.clone(),
                cache: None,
                compression: None,
                stage_claim: None,
                payload_state_digest: None,
            })
        }
        QualificationCacheBackendObservation::Found(_) => {
            Err("normal_tofu_cache_backend_action_mismatch")
        }
        QualificationCacheBackendObservation::NotQueried
        | QualificationCacheBackendObservation::Unavailable => {
            Err("normal_tofu_cache_backend_unavailable")
        }
    }
}

fn validate_cache_hit(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    action: &CacheActionObservation,
    run: GitHubRunContext,
    runtime: super::super::ValidatedTofuRuntime,
    stage_claim: StageReceiptClaim,
    compression: CacheActionCompression,
    observed: QualificationCacheBackendObservation,
) -> Result<NormalTofuProvidersCacheBackendEvidence, &'static str> {
    let context = StageContext::normal_tofu_providers(
        request.runner_temp,
        request.workspace,
        request.cache_key,
    )
    .map_err(|_| "normal_tofu_stage_context_invalid")?;
    let cache = match observed {
        QualificationCacheBackendObservation::Found(cache) => cache,
        QualificationCacheBackendObservation::Absent => {
            return Err("normal_tofu_cache_backend_action_mismatch");
        }
        QualificationCacheBackendObservation::NotQueried
        | QualificationCacheBackendObservation::Unavailable => {
            return Err("normal_tofu_cache_backend_unavailable");
        }
    };
    if cache.id == 0
        || cache.key != request.cache_key
        || cache.git_ref != run.git_ref
        || !super::super::super::cache_action_version::version_agrees_with_action_compression(
            context.action_path(),
            compression,
            &cache.version,
        )?
        || cache.size_bytes == 0
        || cache.size_bytes > max_cache_archive_bytes(request.operation_limit_bytes)?
        || stage_claim.length() > request.operation_limit_bytes
    {
        return Err("normal_tofu_cache_backend_entry_mismatch");
    }
    Ok(NormalTofuProvidersCacheBackendEvidence {
        run,
        target: runtime.target,
        tofu_version: runtime.tofu_version,
        root: runtime.root,
        lockfile_sha256: runtime.lockfile_sha256,
        cache_key: request.cache_key.to_owned(),
        action_hit: action.hit,
        action_matched_key: action.matched_key.clone(),
        cache: Some(cache),
        compression: Some(compression),
        stage_claim: Some(stage_claim),
        payload_state_digest: None,
    })
}

/// Gives a conservative one-file archive ceiling for a bounded raw transfer.
///
/// The pinned `@actions/cache` 6.1.0 `lib/cache.js` records the size of the
/// compressed archive after `createTar` and sends that value as
/// `FinalizeCacheEntryUpload.sizeBytes` (lines 452–495). It therefore differs
/// from the raw stage file length. The policy reserves 1/16 compression
/// expansion and 1 MiB framing headroom after the bounded one-file TAR.
pub(super) fn max_cache_archive_bytes(raw_limit: u64) -> Result<u64, &'static str> {
    if raw_limit == 0 || raw_limit > MAX_CACHE_TRANSFER_BYTES {
        return Err("normal_tofu_cache_archive_limit_invalid");
    }
    raw_limit
        .checked_add(CACHE_ACTION_TAR_OVERHEAD_BYTES)
        .and_then(|tar| tar.checked_add(tar / CACHE_ACTION_COMPRESS_HEADROOM_DENOMINATOR))
        .and_then(|compressed| compressed.checked_add(CACHE_ACTION_COMPRESS_HEADROOM_BYTES))
        .ok_or("normal_tofu_cache_archive_limit_invalid")
}

#[cfg(test)]
#[path = "tests/normal_tofu_restore_transport_tests.rs"]
mod transport_tests;
