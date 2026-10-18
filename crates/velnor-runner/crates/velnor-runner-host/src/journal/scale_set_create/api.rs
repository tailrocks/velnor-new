use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

use super::{
    CreatedScaleSetResponse, ExistingScaleSetObservation, ScaleSetCreateClaim,
    ScaleSetCreateNoEffect, ScaleSetCreateRecord, ScaleSetCreateRequest,
};

impl ConnectionJournal {
    /// Store an exact create intent or give its existing state without authorization.
    pub(crate) async fn prepare_scale_set_create(
        &self,
        request: &ScaleSetCreateRequest,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::prepare(self, request).await
    }

    /// Claim the create effect once for a prepared row.
    pub(crate) async fn claim_scale_set_create(
        &self,
        record: &ScaleSetCreateRecord,
    ) -> Result<Option<ScaleSetCreateClaim>, HostError> {
        super::storage::claim(self, record).await
    }

    /// Record an ambiguous post-claim create result.
    pub(crate) async fn mark_scale_set_create_uncertain(
        &self,
        claim: ScaleSetCreateClaim,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::mark_uncertain(self, claim).await
    }

    /// Record the pinned known no-effect conflict after a claimed POST.
    pub(crate) async fn record_scale_set_create_no_effect(
        &self,
        claim: ScaleSetCreateClaim,
        proof: ScaleSetCreateNoEffect,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::record_no_effect(self, claim, proof).await
    }

    /// Record the exact response from the claimed create request.
    pub(crate) async fn record_scale_set_created(
        &self,
        claim: ScaleSetCreateClaim,
        response: CreatedScaleSetResponse,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::record_created(self, claim, response).await
    }

    /// Adopt an exact GET result when no create effect remains unresolved.
    pub(crate) async fn adopt_scale_set(
        &self,
        request: &ScaleSetCreateRequest,
        observed: ExistingScaleSetObservation,
    ) -> Result<ScaleSetCreateRecord, HostError> {
        super::storage::adopt(self, request, observed).await
    }
}
