// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod identity;
mod inspection;
mod storage;

pub(crate) use identity::{ImageImportIdentity, ImageImportRequest};
pub(crate) use inspection::{ImageImportDescriptor, ImageImportObservation};

use crate::error::HostError;

use super::Journal;
pub(crate) use inspection::ImageImportObservation as ImageImportReceipt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportInspection {
    request: ImageImportRequest,
    observation: ImageImportObservation,
}

impl ImageImportInspection {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageImportState {
    Prepared,

    Requested,

    Uncertain,

    Completed,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageImportId(i64);

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
    pub(crate) async fn prepare_image_import(
        &self,
        request: ImageImportRequest,
    ) -> Result<ImageImportRecord, HostError> {
        storage::prepare(self, request).await
    }

    pub(crate) async fn claim_image_import_effect(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
    ) -> Result<Option<ImageImportClaim>, HostError> {
        storage::claim(self, id, request).await
    }

    pub(crate) async fn mark_image_import_uncertain(
        &self,
        claim: ImageImportClaim,
    ) -> Result<(), HostError> {
        storage::mark_uncertain(self, claim).await
    }

    pub(crate) async fn complete_claimed_image_import(
        &self,
        claim: ImageImportClaim,
        inspection: &ImageImportInspection,
    ) -> Result<(), HostError> {
        storage::complete_claimed(self, claim, inspection).await
    }

    pub(crate) async fn complete_preloaded_image_import(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
        inspection: &ImageImportInspection,
    ) -> Result<(), HostError> {
        storage::complete_preloaded(self, id, request, inspection).await
    }

    pub(crate) async fn latest_completed_worker_image_pair(
        &self,
        engine_id: &str,
        docker_root_digest: &str,
        daemon_architecture: &str,
        platform: &str,
    ) -> Result<Option<(ImageImportRecord, ImageImportRecord)>, HostError> {
        storage::latest_completed_worker_pair(
            self,
            engine_id,
            docker_root_digest,
            daemon_architecture,
            platform,
        )
        .await
    }
}
