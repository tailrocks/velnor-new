//! Check normal `ToFu` restore outputs against runner context and backend state.

use super::NormalTofuProvidersCachePayloadRequest;
use crate::cache_action_stage::{CacheActionCompression, StageContext, StageReceiptClaim};
use crate::qualification_cache_payload::CacheActionObservation;
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
};

const MAX_GITHUB_EVENT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CACHE_TRANSFER_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitHubRunContext {
    pub(super) repository: String,
    pub(super) repository_id: u64,
    pub(super) git_ref: String,
    pub(super) source_sha: String,
    pub(super) workflow_ref: String,
    pub(super) workflow_sha: String,
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
}

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
}

/// Check the action output and exact cache entry before reading archive bytes.
///
/// This gives backend evidence only. It does not verify restored payload bytes
/// or authorize their import.
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
    let runtime = super::validate_request(request)?;
    let run = read_github_run_context()?;
    let observed = crate::qualification_cache_backend::observe_qualification_cache_backend(
        request.workspace,
        &run.repository,
        request.cache_key,
        &run.git_ref,
    )
    .map_err(|_| "normal_tofu_cache_backend_unavailable")?;
    validate_restore_observation(
        request,
        action,
        stage_claim,
        compression,
        run,
        runtime,
        observed,
    )
}

fn validate_restore_observation(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    action: &CacheActionObservation,
    stage_claim: Option<&str>,
    compression: Option<CacheActionCompression>,
    run: GitHubRunContext,
    runtime: super::ValidatedTofuRuntime,
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
    runtime: super::ValidatedTofuRuntime,
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
    runtime: super::ValidatedTofuRuntime,
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
        || !super::super::cache_action_version::version_agrees_with_action_compression(
            context.action_path(),
            compression,
            &cache.version,
        )?
        || cache.size_bytes == 0
        || cache.size_bytes != stage_claim.length()
        || stage_claim.length() > MAX_CACHE_TRANSFER_BYTES
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
    })
}

fn read_github_run_context() -> Result<GitHubRunContext, &'static str> {
    if env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true") {
        return Err("normal_tofu_github_context_invalid");
    }
    let repository_raw = required("GITHUB_REPOSITORY")?;
    let repository = crate::origin::validate_repository_slug(&repository_raw)
        .ok_or("normal_tofu_github_context_invalid")?;
    let repository_id = positive_u64(&required("GITHUB_REPOSITORY_ID")?)?;
    let git_ref = required("GITHUB_REF")?;
    let source_sha = git_sha(&required("GITHUB_SHA")?)?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    if workflow_ref
        .split_once('@')
        .is_none_or(|(workflow, reference)| workflow.is_empty() || reference.is_empty())
    {
        return Err("normal_tofu_github_context_invalid");
    }
    let workflow_sha = git_sha(&required("GITHUB_WORKFLOW_SHA")?)?;
    let run_id = positive_u64(&required("GITHUB_RUN_ID")?)?;
    let run_attempt = u32::try_from(positive_u64(&required("GITHUB_RUN_ATTEMPT")?)?)
        .map_err(|_| "normal_tofu_github_context_invalid")?;
    let event_path = PathBuf::from(
        env::var_os("GITHUB_EVENT_PATH").ok_or("normal_tofu_github_context_invalid")?,
    );
    let event_text = crate::safe_read::read_event_file(&event_path, MAX_GITHUB_EVENT_BYTES)
        .map_err(|_| "normal_tofu_github_event_invalid")?;
    validate_event_repository(&event_text, &repository, repository_id)?;
    Ok(GitHubRunContext {
        repository,
        repository_id,
        git_ref,
        source_sha,
        workflow_ref,
        workflow_sha,
        run_id,
        run_attempt,
    })
}

fn validate_event_repository(
    event_text: &str,
    repository: &str,
    repository_id: u64,
) -> Result<(), &'static str> {
    let event: Value = velnor_actions_contract::parse_strict_json(event_text)
        .map_err(|_| "normal_tofu_github_event_invalid")?;
    let event_repository = event
        .get("repository")
        .and_then(Value::as_object)
        .ok_or("normal_tofu_github_event_invalid")?;
    let event_id = event_repository
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("normal_tofu_github_event_invalid")?;
    let event_name = event_repository
        .get("full_name")
        .and_then(Value::as_str)
        .and_then(crate::origin::validate_repository_slug)
        .ok_or("normal_tofu_github_event_invalid")?;
    if event_id != repository_id || event_name != repository {
        return Err("normal_tofu_github_repository_mismatch");
    }
    Ok(())
}

fn required(name: &str) -> Result<String, &'static str> {
    let value = env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or("normal_tofu_github_context_invalid")?;
    if value.len() > 4_096 || value.chars().any(char::is_control) {
        return Err("normal_tofu_github_context_invalid");
    }
    Ok(value)
}

fn positive_u64(value: &str) -> Result<u64, &'static str> {
    value
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0 && number.to_string() == value)
        .ok_or("normal_tofu_github_context_invalid")
}

fn git_sha(value: &str) -> Result<String, &'static str> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("normal_tofu_github_context_invalid");
    }
    Ok(value.to_owned())
}

#[cfg(test)]
#[path = "tests/normal_tofu_admission_tests.rs"]
mod tests;
