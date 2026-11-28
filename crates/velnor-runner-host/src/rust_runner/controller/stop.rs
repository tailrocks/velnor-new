// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::Run;
use super::github::ACQUIRE_BUDGET;
use crate::launch_blocking::ListenFault;

const JOB_STOP_WAIT: Duration = Duration::from_secs(15);
const ACQUIRE_STOP_SLACK: Duration = Duration::from_secs(2);
const PROOF_WAIT: Duration = Duration::from_secs(30);
const PROOF_RETRY: Duration = Duration::from_secs(1);

impl Run<'_> {
    pub(super) async fn stop(&mut self) -> Result<(), ListenFault> {
        self.stopping = true;
        self.slots.stop_all();
        let started = tokio::time::Instant::now();
        let jobs_deadline = started + JOB_STOP_WAIT;
        while !self.slots.settled() {
            match tokio::time::timeout_at(jobs_deadline, self.tasks.join_next()).await {
                Ok(Some(joined)) => self.handle(joined)?,
                Ok(None) | Err(_) => break,
            }
        }
        let acquires_deadline = started + ACQUIRE_BUDGET + ACQUIRE_STOP_SLACK;
        while self.slots.launching() {
            match tokio::time::timeout_at(acquires_deadline, self.tasks.join_next()).await {
                Ok(Some(joined)) => self.handle(joined)?,
                Ok(None) | Err(_) => break,
            }
        }
        for (reservation, connection) in self.slots.prove_unsettled() {
            eprintln!(
                "rust-runner stopping with reservation {reservation} unsettled; the proof removes runner {}",
                self.runner_name(connection, reservation)
            );
        }
        let proofs_deadline = tokio::time::Instant::now() + PROOF_WAIT;
        let mut retry = tokio::time::interval(PROOF_RETRY);
        retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        while self.slots.proving() {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(proofs_deadline) => break,
                Some(joined) = self.tasks.join_next() => self.handle(joined)?,
                _ = retry.tick() => self.spawn_due_proofs(),
            }
        }
        self.close_sessions().await;
        Ok(())
    }
}
