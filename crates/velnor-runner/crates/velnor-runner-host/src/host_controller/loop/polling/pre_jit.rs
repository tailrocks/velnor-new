// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use crate::{
    error::HostError,
    launch::bootstrap::{
        UncertainRecovery, recover_uncertain_reservations, retire_uncertain_unbound_pairs,
    },
};

use super::PositivePollContext;

const TURN_DEADLINE: Duration = Duration::from_secs(120);
use crate::journal::ConnectionJournal;

pub(super) async fn reconcile(
    scope: &ConnectionJournal,
    pass: usize,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    Box::pin(recover_uncertain_reservations(&UncertainRecovery {
        docker: context.docker,
        journal: context.journal,
        scope,
        engine: context.selected_engine,
        images: context.worker_images,
        image_authority: context.image_authority,
        pass,
    }))
    .await?;
    retire_uncertain_unbound_pairs(
        Instant::now() + TURN_DEADLINE,
        context.docker,
        scope,
        context.selected_engine,
    )
    .await
}
