//! Validate owner layer action and backend observations.

use velnor_actions_contract::workflow::{
    QualificationCacheBackendObservation, QualificationCacheLayerReceipt,
    QualificationCacheRestore, QualificationCacheRestoreResult, QualificationCacheSave,
    QualificationCacheSaveActionResult, QualificationRuntimeIdentity,
};

use super::QualificationCacheBindingOutputs;
use crate::OrchestratorError;
use crate::internal::internal;

/// The runtime facts for one cache owner must be the same before and after cache import.
pub(super) fn validate_layer_receipt(
    receipt: &QualificationCacheLayerReceipt,
    owner: &velnor_actions_contract::workflow::QualificationCacheOwnerDirective,
    expected: &velnor_actions_contract::workflow::QualificationCacheLayerDirective,
    before_runtime: Option<&QualificationRuntimeIdentity>,
    after_runtime: Option<&QualificationRuntimeIdentity>,
    binding: Option<&QualificationCacheBindingOutputs>,
    git_ref: &str,
) -> Result<(), OrchestratorError> {
    if receipt.owner_id != owner.owner_id
        || receipt.platform_id != owner.platform_id
        || receipt.member_digest != owner.member_digest
        || receipt.layer != expected.layer
        || receipt.active != expected.active
        || receipt.identity_digest != expected.identity_digest
    {
        return Err(internal("qualification_cache_owner_layer_mismatch"));
    }
    if expected.active {
        let before =
            before_runtime.ok_or_else(|| internal("qualification_cache_runtime_missing"))?;
        let after = after_runtime
            .ok_or_else(|| internal("qualification_cache_runtime_after_import_missing"))?;
        if before != after || receipt.runtime_identity.as_ref() != Some(after) {
            return Err(internal("qualification_cache_runtime_changed_after_import"));
        }
        if receipt.state_digest.is_none() {
            return Err(internal("qualification_cache_state_missing"));
        }
        let binding = binding.ok_or_else(|| internal("qualification_cache_binding_missing"))?;
        validate_restore_record(&receipt.restore, binding)?;
        validate_save_record(
            &receipt.save,
            receipt.state_digest.as_deref(),
            binding,
            git_ref,
        )?;
    } else if receipt.runtime_identity.is_some() || receipt.state_digest.is_some() {
        return Err(internal("qualification_cache_disabled_layer_has_state"));
    } else if binding.is_some()
        || receipt.restore.result != QualificationCacheRestoreResult::Disabled
        || receipt.restore.requested_key.is_some()
        || receipt.restore.matched_key.is_some()
        || receipt.restore.matched_cache != QualificationCacheBackendObservation::NotQueried
        || receipt.save.action != QualificationCacheSaveActionResult::Disabled
        || receipt.save.requested_key.is_some()
        || receipt.save.before != QualificationCacheBackendObservation::NotQueried
        || receipt.save.after != QualificationCacheBackendObservation::NotQueried
    {
        return Err(internal(
            "qualification_cache_disabled_layer_observed_access",
        ));
    }
    Ok(())
}

fn validate_restore_record(
    restore: &QualificationCacheRestore,
    binding: &QualificationCacheBindingOutputs,
) -> Result<(), OrchestratorError> {
    let key = binding
        .restore_key
        .as_deref()
        .ok_or_else(|| internal("qualification_cache_restore_key_missing"))?;
    let valid = match binding.expected_cache.as_ref() {
        Some(expected) => {
            restore.requested_key.as_deref() == Some(key)
                && restore.matched_key.as_deref() == Some(key)
                && restore.result == QualificationCacheRestoreResult::Hit
                && restore.matched_cache
                    == QualificationCacheBackendObservation::Found(expected.clone())
        }
        None => {
            restore.requested_key.as_deref() == Some(key)
                && restore.matched_key.is_none()
                && restore.result == QualificationCacheRestoreResult::Miss
                && restore.matched_cache == QualificationCacheBackendObservation::Absent
        }
    };
    if !valid {
        return Err(internal("qualification_cache_restore_record_mismatch"));
    }
    Ok(())
}

fn validate_save_record(
    save: &QualificationCacheSave,
    state_digest: Option<&str>,
    binding: &QualificationCacheBindingOutputs,
    git_ref: &str,
) -> Result<(), OrchestratorError> {
    match binding.save_key.as_deref() {
        None => {
            if save.requested_key.is_none()
                && save.action == QualificationCacheSaveActionResult::Disabled
                && save.before == QualificationCacheBackendObservation::NotQueried
                && save.after == QualificationCacheBackendObservation::NotQueried
            {
                return Ok(());
            }
        }
        Some(key) => match save.action {
            QualificationCacheSaveActionResult::NotRequired => {
                if binding.expected_prior_state_digest.as_deref() == state_digest
                    && save.requested_key.is_none()
                    && save.before == QualificationCacheBackendObservation::NotQueried
                    && save.after == QualificationCacheBackendObservation::NotQueried
                {
                    return Ok(());
                }
            }
            QualificationCacheSaveActionResult::Succeeded => {
                if save.requested_key.as_deref() == Some(key)
                    && save.before == QualificationCacheBackendObservation::Absent
                    && matches!(
                        &save.after,
                        QualificationCacheBackendObservation::Found(entry)
                            if entry.id > 0
                                && entry.key == key
                                && entry.git_ref == git_ref
                                && valid_cache_version(&entry.version)
                                && entry.size_bytes > 0
                    )
                {
                    return Ok(());
                }
            }
            QualificationCacheSaveActionResult::Skipped
            | QualificationCacheSaveActionResult::Failed => {
                if save.requested_key.as_deref() == Some(key) {
                    validate_census_key(&save.before, key)?;
                    validate_census_key(&save.after, key)?;
                    return Ok(());
                }
            }
            QualificationCacheSaveActionResult::Disabled => {}
        },
    }
    Err(internal("qualification_cache_save_record_mismatch"))
}

fn validate_census_key(
    observation: &QualificationCacheBackendObservation,
    key: &str,
) -> Result<(), OrchestratorError> {
    match observation {
        QualificationCacheBackendObservation::Absent => Ok(()),
        QualificationCacheBackendObservation::Found(entry)
            if entry.id > 0
                && entry.key == key
                && valid_cache_version(&entry.version)
                && entry.size_bytes > 0 =>
        {
            Ok(())
        }
        _ => Err(internal("qualification_cache_save_census_mismatch")),
    }
}

fn valid_cache_version(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
