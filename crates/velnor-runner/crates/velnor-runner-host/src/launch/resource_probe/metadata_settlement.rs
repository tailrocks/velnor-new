//! This module records a new storage sample after the guest probe is removed.

use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, MetadataSettlementResult},
    launch::SelectedEngineContext,
};

use super::ReleaseReadSecret;
use super::provider::{ImageProvider, ReleaseImageProvider};

mod claim;
mod pre_jit_capacity;
mod sample;
mod setup;

pub(in crate::launch) use pre_jit_capacity::PreJitCapacityObservation;
pub(in crate::launch) use pre_jit_capacity::measure_pre_jit_capacity;

#[cfg(test)]
use sample::verify_startup_evidence;

/// This function measures storage after cleanup. It records the settlement result in the journal.
pub(in crate::launch) async fn settle_storage_metadata_after_cleanup(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    selected: &SelectedEngineContext,
    release_read_secret: &ReleaseReadSecret,
) -> Result<MetadataSettlementResult, HostError> {
    let token = release_read_secret.as_provider_token()?;
    let provider = ReleaseImageProvider::from_compiled_source(token, journal)
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

#[cfg(test)]
pub(in crate::launch) async fn settle_storage_metadata_after_cleanup_with_test_provider(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    selected: &SelectedEngineContext,
    provider: &super::cleanup_observer::CleanupObserverTestProvider,
) -> Result<MetadataSettlementResult, HostError> {
    settle_storage_metadata_after_cleanup_using(
        docker,
        journal,
        scope,
        reservation_id,
        selected,
        provider,
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

#[cfg(test)]
#[path = "metadata_settlement/tests.rs"]
mod tests;
