// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ScopedPrepareContext;

pub(super) async fn retire_pending_pair<P>(
    context: ScopedPrepareContext<'_, P>,
    reservation_id: i64,
) {
    if let Err(error) = super::super::bootstrap::abandon_and_retire_unbound_pair_before(
        context.deadline,
        context.docker,
        context.scoped,
        context.engine,
        reservation_id,
    )
    .await
    {
        eprintln!(
            "host controller kept an unbound pair charged reservation={reservation_id} reason={error}"
        );
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
mod tests;
