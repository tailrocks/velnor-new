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
    journal::Journal,
    launch::{
        ImageAuthority, SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            UncertainRecovery, recover_uncertain_reservations, retire_uncertain_unbound_pairs,
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
                eprintln!(
                    "host controller removed connection held connection={} reason={error}",
                    binding.connection_id
                );
                continue;
            }
        };
        let Ok(recovered) = timeout_at(
            TokioInstant::from_std(startup.deadline),
            Box::pin(recover_uncertain_reservations(&UncertainRecovery {
                docker: startup.docker,
                journal: startup.journal,
                scope: &scope,
                engine: startup.engine,
                images: startup.images,
                image_authority: startup.image_authority,
                pass: 0,
            })),
        )
        .await
        else {
            defer(binding);
            break;
        };
        let retired = retire_uncertain_unbound_pairs(
            startup.deadline,
            startup.docker,
            &scope,
            startup.engine,
        )
        .await;
        if let Err(error) = recovered.and(retired) {
            eprintln!(
                "host controller removed connection held connection={} reason={error}",
                binding.connection_id
            );
        }
    }
}

fn defer(binding: &crate::config::RepositoryConnection) {
    eprintln!(
        "host controller defers removed connection retirement to the next sweep connection={}",
        binding.connection_id
    );
}
