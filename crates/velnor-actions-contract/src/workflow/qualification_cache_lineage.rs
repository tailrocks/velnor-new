//! Typed cache namespaces and receipt lineage for hosted qualification.

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

#[cfg(test)]
#[path = "qualification_cache_lineage_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "qualification_cache_lineage_directive_tests.rs"]
mod directive_tests;

#[cfg(test)]
#[path = "qualification_cache_lineage_admission_tests.rs"]
mod admission_tests;
