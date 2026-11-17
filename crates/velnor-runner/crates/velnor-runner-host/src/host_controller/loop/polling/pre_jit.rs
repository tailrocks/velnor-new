// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tokio::time::{Instant, timeout_at};

use crate::{
    error::HostError,
    launch::bootstrap::{
        INITIALIZER_RECOVERY_BUDGET, UncertainRecovery, UncertainRecoveryWork,
        uncertain_recovery_work,
    },
};

use super::PositivePollContext;

use crate::journal::ConnectionJournal;

pub(super) async fn reconcile(
    scope: &ConnectionJournal,
    pass: usize,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    let deadline = Instant::now() + context.retirement_budget;
    let recovery = UncertainRecovery {
        docker: context.docker,
        journal: context.journal,
        scope,
        engine: context.selected_engine,
        images: context.worker_images,
        image_authority: context.image_authority,
        pass,
    };
    let work = if let Ok(work) = timeout_at(deadline, uncertain_recovery_work(&recovery)).await {
        work?
    } else {
        defer(scope);
        return Ok(());
    };
    let mut initializer_deadline = None;
    for item in work {
        if Instant::now() >= deadline {
            break;
        }
        match item {
            UncertainRecoveryWork::Initializer(initializer) => {
                let until = *initializer_deadline.get_or_insert_with(|| {
                    deadline.min(Instant::now() + INITIALIZER_RECOVERY_BUDGET)
                });
                if Instant::now() < until {
                    initializer.recover_before(&recovery, until, deadline).await;
                }
            }
            UncertainRecoveryWork::AssignedPair(pair) => {
                if timeout_at(deadline, Box::pin(pair.recover(&recovery)))
                    .await
                    .is_err()
                {
                    defer(scope);
                    break;
                }
            }
            UncertainRecoveryWork::UnboundPair(pair) => {
                pair.recover_before(&recovery, deadline).await;
            }
        }
    }
    Ok(())
}

fn defer(scope: &ConnectionJournal) {
    eprintln!(
        "host controller defers pre-JIT recovery to the next sweep connection={}",
        scope.connection_id().as_str()
    );
}
