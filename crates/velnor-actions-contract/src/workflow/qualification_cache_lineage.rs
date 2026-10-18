//! Typed cache namespaces and receipt lineage for hosted qualification.

mod directive;
mod directive_types;
mod identity;
mod receipt;
mod runtime;

#[cfg(test)]
pub(crate) use identity::validate_runtime_identity;

pub use directive::{
    QUALIFICATION_CACHE_DIRECTIVES_OUTPUT, QualificationCacheDirective,
    QualificationCacheLaneDirective, QualificationCacheLayerDirective,
    QualificationCacheRestoreDirective, QualificationCacheRestorePolicy,
    QualificationCacheSavePolicy,
};
pub use identity::{
    BoundQualificationCacheKeys, MAX_QUALIFICATION_CACHE_LANES, QualificationCacheLayer,
    QualificationCacheRestoreExpectation, QualificationCacheSlot,
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityField, QualificationRuntimeIdentityRequirements,
    QualificationRuntimeImageDigests, QualificationRuntimeImageIdentity,
    QualificationRuntimePlatform, QualificationRuntimeScaleSet,
    QualificationRuntimeToolObservation,
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
    qualification_cache_receipt_artifact_name,
};

#[cfg(test)]
#[path = "qualification_cache_lineage_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "qualification_cache_lineage_directive_tests.rs"]
mod directive_tests;

#[cfg(test)]
#[path = "qualification_cache_lineage_admission_tests.rs"]
mod admission_tests;
