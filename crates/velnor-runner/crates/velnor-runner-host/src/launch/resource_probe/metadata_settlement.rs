// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, MetadataSettlementResult},
    launch::SelectedEngineContext,
};

use super::ImageAuthority;
use super::provider::{ImageProvider, ReleaseImageProvider};

mod claim;
mod pre_jit_capacity;
mod sample;
mod setup;

pub(in crate::launch) use pre_jit_capacity::PreJitCapacityObservation;
pub(in crate::launch) use pre_jit_capacity::measure_pre_jit_capacity;

pub(in crate::launch) async fn settle_storage_metadata_after_cleanup(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    selected: &SelectedEngineContext,
    image_authority: &ImageAuthority,
) -> Result<MetadataSettlementResult, HostError> {
    let token = image_authority.as_provider_token()?;
    let provider = ReleaseImageProvider::from_compiled_source(token, journal, selected.endpoint())
        .map_err(|_| HostError::Docker)?;
    settle_storage_metadata_after_cleanup_using(
        docker,
        journal,
        scope,
        reservation_id,
        selected,
        &provider,
    )
    .await
}

async fn settle_storage_metadata_after_cleanup_using<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    selected: &SelectedEngineContext,
    provider: &P,
) -> Result<MetadataSettlementResult, HostError> {
    let _operation = super::readiness::lock_operation(journal).await?;
    if reservation_id <= 0 {
        return Err(HostError::Identity);
    }
    let setup = Box::pin(setup::prepare_settlement_setup(
        docker, journal, scope, selected, provider,
    ))
    .await?;
    let pending = Box::pin(claim::prepare_settlement_claim(
        journal,
        scope,
        reservation_id,
        setup,
    ))
    .await?;
    sample::settle_sample(docker, journal, scope, pending).await
}
