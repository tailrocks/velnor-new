// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;

mod directive;
mod directive_types;
mod identity;
mod receipt;
mod runtime;

pub use directive::{
    QUALIFICATION_CACHE_DIRECTIVES_OUTPUT, QualificationCacheDirective,
    QualificationCacheLaneDirective, QualificationCacheLayerDirective,
    QualificationCacheRestoreDirective, QualificationCacheRestorePolicy,
    QualificationCacheSavePolicy,
};
pub use identity::{
    BoundQualificationCacheKeys, MAX_QUALIFICATION_CACHE_LANES, QualificationCacheLayer,
    QualificationCacheRestoreExpectation, QualificationCacheSlot, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityField, QualificationRuntimeIdentityRequirements,
    QualificationRuntimePlatform,
};
pub use receipt::{
    MAX_QUALIFICATION_RECEIPT_BYTES, MAX_QUALIFICATION_RECEIPT_DEPTH,
    QUALIFICATION_CACHE_RECEIPT_ARTIFACT, QUALIFICATION_CACHE_RECEIPT_FILENAME,
    QualificationCacheAdmission, QualificationCacheArtifact, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation, QualificationCacheLaneReceipt,
    QualificationCacheLayerReceipt, QualificationCacheProducerContext, QualificationCacheReceipt,
    QualificationCacheReceiptArtifactDocument, QualificationCacheReceiptLink,
    QualificationCacheRestore, QualificationCacheRestoreResult, QualificationCacheRunMetadata,
    QualificationCacheSave, QualificationCacheSaveActionResult, QualificationSourceDelta,
};

/// Bounded qualification cache generations: at most two per active
/// family and version, including unverified replacement overlap.
pub const QUALIFICATION_CACHE_MAX_GENERATIONS: u64 = 2;

/// Accept only the shared bounded generation owned by the given MBX
/// version. Run-specific generations mint an unbounded entry per run
/// and evade admission, so they are rejected here.
pub fn validate_qualification_cache_generation(
    generation: &str,
    mbx_version: &str,
) -> Result<(), ContractError> {
    for marker in ["${{", "github.run_id", "github.run_attempt", "github.sha"] {
        if generation.contains(marker) {
            return Err(ContractError::identity(
                "qualification.cache_generation",
                format!("run_specific_generation:{marker}"),
            ));
        }
    }
    if generation.is_empty() || generation.len() > crate::cachekey::MAX_CACHE_KEY_BYTES {
        return Err(ContractError::identity(
            "qualification.cache_generation",
            "invalid_or_too_long",
        ));
    }
    let expected = crate::cachekey::mbx_cache_generation(mbx_version);
    if generation != expected {
        return Err(ContractError::identity(
            "qualification.cache_generation",
            "not_bounded_owned_slot",
        ));
    }
    Ok(())
}
