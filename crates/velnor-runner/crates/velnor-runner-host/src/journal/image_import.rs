//! Durable identity and status for host image imports.

mod identity;
mod inspection;
mod storage;
#[cfg(test)]
mod tests;

pub(crate) use identity::{ImageImportIdentity, ImageImportRequest};
pub(crate) use inspection::{ImageImportDescriptor, ImageImportObservation};

use crate::error::HostError;

use super::Journal;
pub(crate) use inspection::ImageImportObservation as ImageImportReceipt;

/// Verified selected-daemon image inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportInspection {
    request: ImageImportRequest,
    observation: ImageImportObservation,
}

impl ImageImportInspection {
    /// Bind exact store evidence to the selected daemon and request.
    pub(crate) fn verified(
        request: &ImageImportRequest,
        observed_engine_id: &str,
        observed_docker_root_digest: &str,
        observed_daemon_architecture: &str,
        observation: ImageImportObservation,
    ) -> Result<Self, HostError> {
        request.validate()?;
        if request.engine_id != observed_engine_id
            || request.docker_root_digest != observed_docker_root_digest
            || request.daemon_architecture != observed_daemon_architecture
        {
            return Err(HostError::Identity);
        }
        observation.validate_for(request)?;
        Ok(Self {
            request: request.clone(),
            observation,
        })
    }

    pub(crate) fn image_id(&self) -> &str {
        self.observation.image_id()
    }

    pub(super) fn receipt(&self) -> Result<String, HostError> {
        self.observation.receipt()
    }
}

/// Durable image-import lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageImportState {
    /// Intent exists; no import effect was claimed.
    Prepared,
    /// One-shot Docker import claim committed; effect may be in progress.
    Requested,
    /// Docker import response was ambiguous.
    Uncertain,
    /// Exact image was verified on the selected daemon.
    Completed,
    /// Intent was safely abandoned before any effect claim.
    Aborted,
}

impl ImageImportState {
    pub(super) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "completed" => Ok(Self::Completed),
            "aborted" => Ok(Self::Aborted),
            _ => Err(HostError::Journal),
        }
    }
}

/// Stable row identity for one import attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageImportId(i64);

/// In-memory authority for the one live import request claimed by this caller.
#[derive(Debug)]
pub(crate) struct ImageImportClaim {
    journal: Journal,
    id: ImageImportId,
    request: ImageImportRequest,
}

impl ImageImportClaim {
    pub(super) fn new(journal: &Journal, id: ImageImportId, request: &ImageImportRequest) -> Self {
        Self {
            journal: journal.clone(),
            id,
            request: request.clone(),
        }
    }
}

/// Durable import attempt returned by prepare and read operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportRecord {
    pub(crate) id: ImageImportId,
    pub(crate) attempt: i64,
    pub(crate) state: ImageImportState,
    pub(crate) request: ImageImportRequest,
    pub(crate) image_id: Option<String>,
    pub(crate) receipt: Option<ImageImportReceipt>,
}

impl Journal {
    /// Prepare an exact import or replay its latest durable attempt.
    pub(crate) async fn prepare_image_import(
        &self,
        request: ImageImportRequest,
    ) -> Result<ImageImportRecord, HostError> {
        storage::prepare(self, request).await
    }

    /// Claim the import effect once for the exact request and selected engine.
    pub(crate) async fn claim_image_import_effect(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
    ) -> Result<Option<ImageImportClaim>, HostError> {
        storage::claim(self, id, request).await
    }

    /// Mark a post-claim Docker result uncertain; uncertainty cannot be retried.
    pub(crate) async fn mark_image_import_uncertain(
        &self,
        claim: ImageImportClaim,
    ) -> Result<(), HostError> {
        storage::mark_uncertain(self, claim).await
    }

    /// Complete the claimed load after its stream succeeds and inspection matches.
    pub(crate) async fn complete_claimed_image_import(
        &self,
        claim: ImageImportClaim,
        inspection: &ImageImportInspection,
    ) -> Result<(), HostError> {
        storage::complete_claimed(self, claim, inspection).await
    }

    /// Complete a verified preloaded image only before any import claim.
    pub(crate) async fn complete_preloaded_image_import(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
        inspection: &ImageImportInspection,
    ) -> Result<(), HostError> {
        storage::complete_preloaded(self, id, request, inspection).await
    }

    /// Abandon only an intent whose one-shot effect was never claimed.
    #[cfg(test)]
    pub(crate) async fn abort_image_import_before_effect(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
    ) -> Result<(), HostError> {
        storage::abort_before_effect(self, id, request).await
    }
}
