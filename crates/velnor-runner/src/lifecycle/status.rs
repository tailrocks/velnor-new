// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::time::Duration;

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::exec::StepResult;
use crate::protocol::{StepConclusion, TwirpResultsClient, TwirpStep};

pub(super) struct StatusQueue {
    updates: UnboundedSender<Vec<TwirpStep>>,
    task: JoinHandle<()>,
    budget: Duration,
}

impl StatusQueue {
    pub(super) fn spawn(
        results: TwirpResultsClient,
        plan_id: String,
        job_id: String,
        budget: Duration,
    ) -> Self {
        let (updates, mut queue) = unbounded_channel::<Vec<TwirpStep>>();
        let task = tokio::spawn(async move {
            let mut order = 0_i64;
            while let Some(steps) = queue.recv().await {
                order += 1;
                let sent = timeout(
                    budget,
                    results.update_steps(&steps, &plan_id, &job_id, order),
                )
                .await;
                match sent {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => eprintln!("warning: update step status: {error:#}"),
                    Err(_) => eprintln!("warning: update step status: timed out"),
                }
            }
        });
        Self {
            updates,
            task,
            budget,
        }
    }

    pub(super) fn push(&self, steps: Vec<TwirpStep>) {
        let _ = self.updates.send(steps);
    }

    pub(super) async fn finish(self) {
        drop(self.updates);
        let mut task = self.task;
        if timeout(self.budget, &mut task).await.is_err() {
            task.abort();
            eprintln!("warning: step status updates were not delivered in time and are dropped");
        }
    }
}

pub(super) fn conclusion(result: StepResult) -> StepConclusion {
    match result {
        StepResult::Succeeded => StepConclusion::Success,
        StepResult::Failed => StepConclusion::Failure,
        StepResult::Skipped => StepConclusion::Skipped,
        StepResult::Cancelled => StepConclusion::Cancelled,
    }
}
