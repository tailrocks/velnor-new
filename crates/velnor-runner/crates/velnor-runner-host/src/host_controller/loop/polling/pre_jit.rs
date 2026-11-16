// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tokio::time::{Instant, timeout_at};

use crate::{
    error::HostError,
    launch::bootstrap::{
        UncertainRecovery, recover_uncertain_reservations, retire_uncertain_unbound_pairs,
    },
};

use super::PositivePollContext;

use crate::journal::ConnectionJournal;

pub(super) async fn reconcile(
    scope: &ConnectionJournal,
    pass: usize,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    let deadline = std::time::Instant::now() + context.retirement_budget;
    let result = timeout_at(Instant::from_std(deadline), async {
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
        retire_uncertain_unbound_pairs(deadline, context.docker, scope, context.selected_engine)
            .await
    })
    .await;
    if let Ok(result) = result {
        result
    } else {
        eprintln!(
            "host controller defers pre-JIT recovery to the next sweep connection={}",
            scope.connection_id().as_str()
        );
        Ok(())
    }
}
