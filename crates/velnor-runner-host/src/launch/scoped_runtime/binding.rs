// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BoundScopedBootstrap, HostError, ImageProvider, PendingBootstrapPair,
    PreparedAcquiredBootstrap, RecordedStorageNetwork, ScopedPrepareContext, ScopedRuntimeOutcome,
    StartedBootstrapPair, VerifiedMetadataInitialization, VerifiedStorageInitializationReady,
};

pub(super) async fn cleanup_failed_pair<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    error: HostError,
    pending_pair: Option<PendingBootstrapPair>,
    reservation_id: i64,
    context: ScopedPrepareContext<'_, P>,
) -> ScopedRuntimeOutcome {
    let outcome = Box::pin(
        super::super::bootstrap::remove_bootstrap_after_terminal_failure(
            context.docker,
            context.scoped,
            context.host,
            context.engine,
            context.images,
            reservation_id,
            context.provider,
        ),
    )
    .await;
    let cleaned = cleaned_or_logged(reservation_id, outcome, |line| eprintln!("{line}"));
    super::pending(
        prepared,
        Some(network),
        None,
        if cleaned { None } else { pending_pair },
        error,
    )
}

fn cleaned_or_logged(
    reservation_id: i64,
    outcome: Result<bool, HostError>,
    log: impl FnOnce(String),
) -> bool {
    match outcome {
        Ok(cleaned) => cleaned,
        Err(cleanup) => {
            log(super::cleanup_failure_line(reservation_id, cleanup));
            false
        }
    }
}

pub(super) async fn bind_started<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    started: StartedBootstrapPair,
    metadata: VerifiedMetadataInitialization,
    initialization: VerifiedStorageInitializationReady,
    context: ScopedPrepareContext<'_, P>,
) -> Result<ScopedRuntimeOutcome, HostError> {
    let observation_context = super::super::bootstrap::StorageObservationContext::new(
        context.docker,
        context.scoped,
        context.host,
        context.provider,
        context.engine,
        context.images,
    );
    let binding = match super::super::bootstrap::observe_storage_resources_with_provider(
        &observation_context,
        &prepared,
        &started,
        &initialization,
    )
    .await
    {
        Ok(binding) => binding,
        Err(error) => {
            return Ok(super::pending(
                prepared,
                Some(network),
                Some(started),
                None,
                error,
            ));
        }
    };
    let guest_boot_id = binding.guest_boot_id().to_owned();
    if let Err(error) = context
        .scoped
        .bind_storage_resources(prepared.acquired().reservation_id(), &binding)
        .await
    {
        return Ok(super::pending(
            prepared,
            Some(network),
            Some(started),
            None,
            error,
        ));
    }
    Ok(ScopedRuntimeOutcome::Bound(Box::new(
        BoundScopedBootstrap {
            prepared,
            _network: network,
            started,
            metadata,
            initialization,
            binding,
            guest_boot_id,
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::cleaned_or_logged;
    use crate::error::HostError;

    #[test]
    fn a_failed_pair_cleanup_is_logged_and_not_reported_as_cleaned() {
        let mut logged = Vec::new();
        let cleaned = cleaned_or_logged(9, Err(HostError::Journal), |line| logged.push(line));
        assert!(!cleaned);
        assert_eq!(
            logged,
            ["host controller pair cleanup failed reservation=9 reason=journal"]
        );
    }

    #[test]
    fn a_clean_pair_cleanup_is_not_logged() {
        let mut logged = Vec::new();
        assert!(cleaned_or_logged(9, Ok(true), |line| logged.push(line)));
        assert_eq!(logged, Vec::<String>::new());
    }
}
