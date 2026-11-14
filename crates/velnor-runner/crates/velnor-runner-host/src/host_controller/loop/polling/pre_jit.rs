// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
    launch::bootstrap::{UncertainRecovery, recover_uncertain_reservations},
};

use super::super::state::PreparedConnection;
use super::PositivePollContext;

pub(super) async fn reconcile(
    prepared: &PreparedConnection,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    Box::pin(recover_uncertain_reservations(&UncertainRecovery {
        docker: context.docker,
        journal: context.journal,
        scope: &prepared.scope,
        engine: context.selected_engine,
        images: context.worker_images,
        image_authority: context.image_authority,
    }))
    .await
}
