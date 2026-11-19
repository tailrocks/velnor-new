// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{config::RepositoryConnection, error::HostError};

use super::{ConnectionPollContext, PreparedConnection, positive_poll_context};

pub(super) fn pending(
    prepared: &PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
) -> bool {
    context
        .config
        .connection(&prepared.binding.connection_id)
        .is_none()
        && pending_retirements
            .iter()
            .any(|binding| prepared.is_for(binding))
}

pub(super) async fn reconcile(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
) -> Result<(), HostError> {
    let positive = positive_poll_context(context);
    Box::pin(super::super::super::polling::reconcile_removed_pre_jit(
        prepared, &positive,
    ))
    .await
}
