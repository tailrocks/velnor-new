//! Do a check of the cache entry and local byte limit before SDK download.

use super::super::NormalTofuProvidersCachePayloadRequest;
use super::context::{GitHubRunContext, read_github_run_context};
use crate::cache_action_stage::{CacheActionCompression, StageContext};
use crate::qualification_cache_payload::storage_admission::{
    CacheTreeUsage, FreshCapacity, VerifiedCacheBudget, observe_cache_storage, read_verified_budget,
};
use crate::qualification_cache_payload::transfer::source::SourceDirectory;
use std::env;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
};

/// Backend and storage facts checked before a restore action may download bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalTofuProvidersCacheRestorePreflight {
    cache_key: String,
    repository: String,
    git_ref: String,
    cache_entry: Option<QualificationCacheBackendEntry>,
    compression: Option<CacheActionCompression>,
    max_download_bytes: Option<u64>,
    max_raw_stage_bytes: Option<u64>,
}

impl NormalTofuProvidersCacheRestorePreflight {
    /// This method returns the cache key used in the cache service query.
    #[must_use]
    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    /// This method returns the repository used in the cache service query.
    #[must_use]
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// This method returns the ref used in the cache service query.
    #[must_use]
    pub fn git_ref(&self) -> &str {
        &self.git_ref
    }

    /// This method returns the one cache object found before restore.
    #[must_use]
    pub fn cache_entry(&self) -> Option<&QualificationCacheBackendEntry> {
        self.cache_entry.as_ref()
    }

    /// This method returns the compression method that matches the cache version in the action path.
    #[must_use]
    pub const fn compression(&self) -> Option<CacheActionCompression> {
        self.compression
    }

    /// This method returns the maximum compressed size allowed before restore.
    #[must_use]
    pub const fn max_download_bytes(&self) -> Option<u64> {
        self.max_download_bytes
    }

    /// This method returns the maximum size of one uncompressed file after restore.
    #[must_use]
    pub const fn max_raw_stage_bytes(&self) -> Option<u64> {
        self.max_raw_stage_bytes
    }
}

/// Do a check of the cache service identity and storage limits before SDK download.
///
/// The returned value contains facts from before download. Import still does a check
/// of the cache object, action output, previous import record, compression method, and payload bytes.
///
/// # Errors
///
/// This method returns an error code when runner context, cache service data, projection,
/// filesystem, cache object, or capacity values conflict.
pub fn preflight_normal_tofu_providers_cache_restore(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
) -> Result<NormalTofuProvidersCacheRestorePreflight, &'static str> {
    if request.previous_stage_claim.is_some() {
        return Err("normal_tofu_restore_stage_claim_unexpected");
    }
    super::super::validate_request(request)?;
    let run = read_github_run_context()?;
    let observed = crate::qualification_cache_backend::observe_qualification_cache_backend(
        request.workspace,
        &run.repository,
        request.cache_key,
        &run.git_ref,
    )
    .map_err(|_| "normal_tofu_cache_backend_unavailable")?;
    let entry = match observed {
        QualificationCacheBackendObservation::Absent => {
            return Ok(NormalTofuProvidersCacheRestorePreflight {
                cache_key: request.cache_key.to_owned(),
                repository: run.repository,
                git_ref: run.git_ref,
                cache_entry: None,
                compression: None,
                max_download_bytes: None,
                max_raw_stage_bytes: None,
            });
        }
        QualificationCacheBackendObservation::Found(entry) => entry,
        QualificationCacheBackendObservation::NotQueried
        | QualificationCacheBackendObservation::Unavailable => {
            return Err("normal_tofu_cache_backend_unavailable");
        }
    };

    let runner_name = env::var("RUNNER_NAME")
        .ok()
        .filter(|name| !name.is_empty())
        .ok_or("cache_storage_runner_binding_mismatch")?;
    let budget = read_verified_budget(request.runner_temp, &runner_name)?;
    let work_anchor = SourceDirectory::open(request.runner_temp)
        .map_err(|_| "cache_storage_work_root_invalid")?;
    let live_path = request.runner_temp.join("velnor/cache");
    let stage_path = request.runner_temp.join("velnor/cache-staging");
    let live = SourceDirectory::open_optional(&live_path)
        .map_err(|_| "cache_storage_live_root_invalid")?;
    let stage = SourceDirectory::open_optional(&stage_path)
        .map_err(|_| "cache_storage_stage_root_invalid")?;
    let context = StageContext::normal_tofu_providers(
        request.runner_temp,
        request.workspace,
        request.cache_key,
    )
    .map_err(|_| "normal_tofu_stage_context_invalid")?;
    let stage_parent = context
        .action_path()
        .parent()
        .ok_or("normal_tofu_stage_path_invalid")?;
    if let Some(parent) = SourceDirectory::open_optional(stage_parent)
        .map_err(|_| "normal_tofu_stage_path_invalid")?
    {
        match rustix::fs::statat(
            parent.fd(),
            c"payload",
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(_) => return Err("normal_tofu_stage_path_not_empty"),
            Err(error) if error == rustix::io::Errno::NOENT => {}
            Err(_) => return Err("normal_tofu_stage_path_invalid"),
        }
    }
    let (live_usage, stage_usage, fresh) =
        observe_cache_storage(&budget, &work_anchor, live.as_ref(), stage.as_ref())?;
    preflight_found_entry(
        request,
        &run,
        entry,
        &budget,
        live_usage,
        stage_usage,
        fresh,
    )
}

fn preflight_found_entry(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
    run: &GitHubRunContext,
    entry: QualificationCacheBackendEntry,
    budget: &VerifiedCacheBudget,
    live_usage: CacheTreeUsage,
    stage_usage: CacheTreeUsage,
    fresh: FreshCapacity,
) -> Result<NormalTofuProvidersCacheRestorePreflight, &'static str> {
    if entry.id == 0 || entry.key != request.cache_key || entry.git_ref != run.git_ref {
        return Err("normal_tofu_cache_backend_entry_mismatch");
    }
    let context = StageContext::normal_tofu_providers(
        request.runner_temp,
        request.workspace,
        request.cache_key,
    )
    .map_err(|_| "normal_tofu_stage_context_invalid")?;
    let compression = super::super::super::cache_action_version::compression_for_action_version(
        context.action_path(),
        &entry.version,
    )
    .map_err(|_| "normal_tofu_cache_backend_entry_mismatch")?;
    let operation_limit = budget.operation_limit(
        request.operation_limit_bytes,
        live_usage.bytes,
        live_usage.inodes,
        stage_usage.bytes,
        stage_usage.inodes,
    )?;
    let limits = budget.restore_limits(
        operation_limit,
        entry.size_bytes,
        live_usage,
        stage_usage,
        fresh,
    )?;
    budget.validate_import_growth(
        live_usage.bytes,
        live_usage.inodes,
        limits.live_growth_bytes(),
        limits.live_growth_inodes(),
        fresh.available_bytes,
        fresh.available_inodes,
    )?;
    Ok(NormalTofuProvidersCacheRestorePreflight {
        cache_key: request.cache_key.to_owned(),
        repository: run.repository.clone(),
        git_ref: run.git_ref.clone(),
        cache_entry: Some(entry),
        compression: Some(compression),
        max_download_bytes: Some(limits.max_download_bytes()),
        max_raw_stage_bytes: Some(limits.raw_stage_bytes()),
    })
}

#[cfg(test)]
#[path = "tests/normal_tofu_admission_preflight_tests.rs"]
mod tests;
