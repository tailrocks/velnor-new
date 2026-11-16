// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use bollard::Docker;

use crate::{
    config::HostConfig,
    journal::Journal,
    launch::{
        ImageAuthority, SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{UncertainRecovery, recover_uncertain_reservations, retire_uncertain_unbound_pairs},
    },
};

pub(crate) struct RemovedStartup<'a> {
    pub(crate) state: &'a Path,
    pub(crate) config: &'a HostConfig,
    pub(crate) journal: &'a Journal,
    pub(crate) docker: &'a Docker,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) image_authority: &'a ImageAuthority,
}

pub(crate) async fn reconcile_removed_at_startup(
    startup: &RemovedStartup<'_>,
) {
    let mut removed = Vec::new();
    super::discover_retirements(startup.state, startup.config, &mut removed);
    for binding in &removed {
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
        let recovered = Box::pin(recover_uncertain_reservations(&UncertainRecovery {
            docker: startup.docker,
            journal: startup.journal,
            scope: &scope,
            engine: startup.engine,
            images: startup.images,
            image_authority: startup.image_authority,
            pass: 0,
        }))
        .await;
        let retired = retire_uncertain_unbound_pairs(startup.docker, &scope, startup.engine).await;
        if let Err(error) = recovered.and(retired) {
            eprintln!(
                "host controller removed connection held connection={} reason={error}",
                binding.connection_id
            );
        }
    }
}
