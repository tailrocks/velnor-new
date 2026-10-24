use super::super::StageError;
use super::super::compression::{ArchiveCompression, CacheActionCompression};
use super::super::receipt::StageReceiptClaim;

/// Gives the verified stage observation produced by an extract operation.
#[derive(Debug)]
pub struct TarOutcome {
    /// For an extraction operation, this field contains a new compact claim.
    /// For other operations, this field contains `None`.
    pub receipt: Option<StageReceiptClaim>,
    /// Exact cache SDK compression identity parsed from this invocation.
    /// `None` means the invocation did not identify a supported compressor.
    pub compression: Option<CacheActionCompression>,
}

impl TarOutcome {
    pub(super) const fn without_observation() -> Self {
        Self {
            receipt: None,
            compression: None,
        }
    }

    pub(super) fn observed(
        receipt: Option<StageReceiptClaim>,
        compression: ArchiveCompression,
    ) -> Result<Self, StageError> {
        Ok(Self {
            receipt,
            compression: Some(CacheActionCompression::try_from(compression)?),
        })
    }
}
