// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::time::Duration;

use bollard::errors::Error as DockerError;
use tokio::task::JoinHandle;

use crate::error::HostError;

use super::deadline;
use super::resources::OwnedResources;

#[derive(Clone)]
pub(super) enum MutationKind {
    Container(String),
    Volume(String),
}

#[derive(Clone)]
pub(super) enum MutationValue {
    ContainerId(String),
    Volume,
}

#[derive(Clone)]
pub(super) enum MutationResult {
    Created(MutationValue),
    Rejected(HostError),
    Uncertain(HostError),
}

pub(super) struct PendingMutation {
    pub(super) identity: String,
    pub(super) kind: MutationKind,
    pub(super) task: Option<JoinHandle<MutationResult>>,
    pub(super) outcome: Option<MutationResult>,
}

pub(super) enum MutationWait {
    Settled(MutationResult),
    Pending,
}

pub(super) fn spawn<F>(identity: String, kind: MutationKind, future: F) -> PendingMutation
where
    F: Future<Output = MutationResult> + Send + 'static,
{
    PendingMutation {
        identity,
        kind,
        task: Some(tokio::spawn(future)),
        outcome: None,
    }
}

pub(super) fn uncertain_docker(error: &DockerError) -> MutationResult {
    if matches!(
        error,
        DockerError::DockerResponseServerError {
            status_code: 400..=499,
            ..
        }
    ) {
        MutationResult::Rejected(HostError::Docker)
    } else {
        MutationResult::Uncertain(HostError::Docker)
    }
}

pub(super) fn uncertain(error: HostError) -> MutationResult {
    MutationResult::Uncertain(error)
}

pub(super) async fn wait(mutation: &mut PendingMutation, timeout: Duration) -> MutationWait {
    if let Some(outcome) = &mutation.outcome {
        return MutationWait::Settled(outcome.clone());
    }
    let Some(task) = mutation.task.as_mut() else {
        let outcome = MutationResult::Uncertain(HostError::Docker);
        mutation.outcome = Some(outcome.clone());
        return MutationWait::Settled(outcome);
    };
    match deadline(task, timeout).await {
        Ok(joined) => {
            mutation.task = None;
            let outcome = joined.unwrap_or(MutationResult::Uncertain(HostError::Docker));
            mutation.outcome = Some(outcome.clone());
            MutationWait::Settled(outcome)
        }
        Err(_) => MutationWait::Pending,
    }
}

impl OwnedResources {
    pub(super) async fn await_mutation(
        &mut self,
        mut mutation: PendingMutation,
        timeout: Duration,
    ) -> Result<MutationValue, HostError> {
        match wait(&mut mutation, timeout).await {
            MutationWait::Settled(MutationResult::Created(value)) => Ok(value),
            MutationWait::Settled(MutationResult::Rejected(error)) => Err(error),
            MutationWait::Settled(MutationResult::Uncertain(error)) => {
                self.pending_mutations.push(mutation);
                Err(error)
            }
            MutationWait::Pending => {
                self.pending_mutations.push(mutation);
                Err(HostError::DockerTimeout)
            }
        }
    }
}
