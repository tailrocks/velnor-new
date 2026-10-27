// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_sources::SourceByteLedger;
use crate::error::MiseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTransferDirection {
    OriginDownload,
    CacheTransfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTransferOutcome {
    Completed,
    Failed,
    NotAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTransfer {
    pub direction: SourceTransferDirection,
    pub object_id: String,
    pub transferred_bytes: u64,
    pub outcome: SourceTransferOutcome,
}

impl SourceTransfer {
    pub fn validate(&self) -> Result<(), MiseError> {
        if self.object_id.is_empty() || self.object_id.contains('\0') {
            return Err(MiseError::InvalidStepInput {
                field: "transfer_object_id".to_owned(),
                value: self.object_id.clone(),
            });
        }
        Ok(())
    }
}

pub fn apply_source_transfer(
    ledger: &mut SourceByteLedger,
    transfer: &SourceTransfer,
) -> Result<(), MiseError> {
    transfer.validate()?;
    ledger.validate()?;
    if transfer.outcome != SourceTransferOutcome::Completed {
        return Ok(());
    }
    match transfer.direction {
        SourceTransferDirection::OriginDownload => ledger.record_origin(transfer.transferred_bytes),
        SourceTransferDirection::CacheTransfer => ledger.record_cache(transfer.transferred_bytes),
    }
}
