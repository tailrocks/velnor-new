//! Bounded cache receipt model and validation.

mod admission;
mod completed;
mod errors;
mod lineage;
mod source_delta;
mod types;
mod validation;

pub use admission::QualificationCacheAdmission;
pub use types::{
    MAX_QUALIFICATION_RECEIPT_BYTES, MAX_QUALIFICATION_RECEIPT_DEPTH,
    QUALIFICATION_CACHE_RECEIPT_ARTIFACT, QUALIFICATION_CACHE_RECEIPT_FILENAME,
    QualificationCacheArtifact, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation, QualificationCacheLaneReceipt,
    QualificationCacheLayerReceipt, QualificationCacheProducerContext, QualificationCacheReceipt,
    QualificationCacheReceiptArtifactDocument, QualificationCacheReceiptLink,
    QualificationCacheRestore, QualificationCacheRestoreResult, QualificationCacheRunMetadata,
    QualificationCacheSave, QualificationCacheSaveActionResult, QualificationSourceDelta,
};
pub(super) fn layer_applies(
    entry: &crate::workflow::MatrixEntry,
    layer: super::identity::QualificationCacheLayer,
) -> bool {
    lineage::layer_applies(entry, layer)
}
