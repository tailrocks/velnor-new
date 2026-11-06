// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

use super::{
    CreatedScaleSetResponse, ExistingScaleSetObservation, ScaleSetCreateClaim,
    ScaleSetCreateNoEffect, ScaleSetCreateRecord, ScaleSetCreateRequest,
};

impl ConnectionJournal {
    pub(crate) async fn prepare_scale_set_create(
        &self,
        request: &ScaleSetCreateRequest,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::prepare(self, request).await
    }

    pub(crate) async fn claim_scale_set_create(
        &self,
        record: &ScaleSetCreateRecord,
    ) -> Result<Option<ScaleSetCreateClaim>, HostError> {
        super::storage::claim(self, record).await
    }

    pub(crate) async fn mark_scale_set_create_uncertain(
        &self,
        claim: ScaleSetCreateClaim,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::mark_uncertain(self, claim).await
    }

    pub(crate) async fn record_scale_set_create_no_effect(
        &self,
        claim: ScaleSetCreateClaim,
        proof: ScaleSetCreateNoEffect,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::record_no_effect(self, claim, proof).await
    }

    pub(crate) async fn record_scale_set_created(
        &self,
        claim: ScaleSetCreateClaim,
        response: CreatedScaleSetResponse,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::record_created(self, claim, response).await
    }

    pub(crate) async fn adopt_scale_set(
        &self,
        request: &ScaleSetCreateRequest,
        observed: ExistingScaleSetObservation,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::adopt(self, request, observed).await
    }
}
