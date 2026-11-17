// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    path::Path,
    time::{Duration, Instant},
};

use bollard::Docker;
use tokio::time::{Instant as TokioInstant, timeout_at};

use crate::{
    config::HostConfig,
    error::HostError,
    journal::Journal,
    launch::{
        ImageAuthority, SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            INITIALIZER_RECOVERY_BUDGET, UncertainRecovery, UncertainRecoveryWork,
            retire_uncertain_unbound_pairs, uncertain_recovery_work,
        },
    },
};

pub(crate) const REMOVED_STARTUP_BUDGET: Duration = Duration::from_secs(120);

pub(crate) struct RemovedStartup<'a> {
    pub(crate) state: &'a Path,
    pub(crate) config: &'a HostConfig,
    pub(crate) journal: &'a Journal,
    pub(crate) docker: &'a Docker,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) image_authority: &'a ImageAuthority,
    pub(crate) deadline: Instant,
}

pub(crate) async fn reconcile_removed_at_startup(startup: &RemovedStartup<'_>) {
    let mut removed = Vec::new();
    super::discover_retirements(startup.state, startup.config, &mut removed);
    for binding in &removed {
        if Instant::now() >= startup.deadline {
            defer(binding);
            break;
        }
        let scope = match startup.journal.existing_connection_scope(binding).await {
            Ok(scope) => scope,
            Err(error) => {
                held(binding, error);
                continue;
            }
        };
        let recovery = UncertainRecovery {
            docker: startup.docker,
            journal: startup.journal,
            scope: &scope,
            engine: startup.engine,
            images: startup.images,
            image_authority: startup.image_authority,
            pass: 0,
        };
        let discovered = timeout_at(
            TokioInstant::from_std(startup.deadline),
            uncertain_recovery_work(&recovery),
        )
        .await;
        match discovered {
            Ok(Ok(work)) => recover_work(work, &recovery, startup.deadline).await,
            Ok(Err(error)) => {
                let retired = retire_uncertain_unbound_pairs(
                    startup.deadline,
                    startup.docker,
                    &scope,
                    startup.engine,
                )
                .await;
                held(binding, retired.err().unwrap_or(error));
            }
            Err(_) => {
                defer(binding);
                break;
            }
        }
    }
}

async fn recover_work(
    work: Vec<UncertainRecoveryWork>,
    recovery: &UncertainRecovery<'_>,
    deadline: Instant,
) {
    let mut initializer_deadline = None;
    for item in work {
        if Instant::now() >= deadline {
            break;
        }
        match item {
            UncertainRecoveryWork::Initializer(initializer) => {
                let until = *initializer_deadline.get_or_insert_with(|| {
                    TokioInstant::from_std(deadline)
                        .min(TokioInstant::now() + INITIALIZER_RECOVERY_BUDGET)
                });
                if TokioInstant::now() < until {
                    initializer.recover_before(recovery, until, deadline).await;
                }
            }
            UncertainRecoveryWork::AssignedPair(pair) => {
                if timeout_at(
                    TokioInstant::from_std(deadline),
                    Box::pin(pair.recover(recovery)),
                )
                .await
                .is_err()
                {
                    break;
                }
            }
            UncertainRecoveryWork::UnboundPair(pair) => {
                pair.recover_before(recovery, deadline).await;
            }
        }
    }
}

fn held(binding: &crate::config::RepositoryConnection, error: HostError) {
    eprintln!(
        "host controller removed connection held connection={} reason={error}",
        binding.connection_id
    );
}

fn defer(binding: &crate::config::RepositoryConnection) {
    eprintln!(
        "host controller defers removed connection retirement to the next sweep connection={}",
        binding.connection_id
    );
}
