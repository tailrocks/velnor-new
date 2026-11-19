// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use std::time::Instant;
use velnor_runner_github::{EncodedJit, RunnerReference, Transport, jit, jit_request};

use crate::{journal::ConnectionJournal, launch::Lane, scale_set::EnsureError};

use super::BoundScopedBootstrap;
use crate::launch::VerifiedWorkerImages;
use crate::worker::execution_authorization::{
    ProofChannelMeasurement, VerifiedProofChannelBinding, measure_proof_channel,
};

#[must_use]
pub(crate) struct JitReadyBootstrap {
    bound: BoundScopedBootstrap,
    runner: RunnerReference,
    jit: EncodedJit,
}

#[must_use]
pub(crate) struct ChannelReadyBootstrap {
    bound: BoundScopedBootstrap,

    jit: EncodedJit,
    channel: VerifiedProofChannelBinding,
}

#[must_use]
pub(crate) struct PendingJitBootstrap {
    bound: Box<BoundScopedBootstrap>,
}

#[must_use]
pub(crate) struct RunnerReadyBootstrap {
    bound: BoundScopedBootstrap,

    channel: VerifiedProofChannelBinding,
}

impl RunnerReadyBootstrap {
    pub(crate) fn request_id(&self) -> Option<i64> {
        self.bound.prepared.acquired().request().request_id()
    }

    pub(crate) fn intent_id(&self) -> i64 {
        self.bound.prepared.acquired().intent_id()
    }

    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }

    pub(crate) fn channel(&self) -> &VerifiedProofChannelBinding {
        &self.channel
    }
}

impl PendingJitBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }
}

pub(crate) async fn create_and_bind<T>(
    bound: BoundScopedBootstrap,
    lane: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
) -> Result<JitReadyBootstrap, PendingJitBootstrap>
where
    T: Transport + Lane,
{
    let acquired = bound.prepared.acquired();
    let Some(request_id) = acquired.request().request_id() else {
        return Err(PendingJitBootstrap {
            bound: Box::new(bound),
        });
    };
    let runner_name = acquired.runner_name().to_owned();
    let intent_id = acquired.intent_id();
    let reservation_id = acquired.reservation_id();
    let scale_set_id = acquired.request().scale_set_id();
    let policy_generation = acquired.authority_generation();
    let policy_digest = acquired.authority_digest().to_owned();
    let result = match request_and_bind(
        lane,
        admin_token,
        scoped,
        JitRequestIdentity {
            intent_id,
            reservation_id,
            request_id,
            runner_name: &runner_name,
            scale_set_id,
            policy_generation,
            policy_digest: &policy_digest,
        },
    )
    .await
    {
        Ok(result) => result,
        Err(failure) => {
            let _ = &failure;
            return Err(PendingJitBootstrap {
                bound: Box::new(bound),
            });
        }
    };
    Ok(JitReadyBootstrap {
        bound,
        runner: result.0,
        jit: result.1,
    })
}

struct JitRequestIdentity<'a> {
    intent_id: i64,
    reservation_id: i64,
    request_id: i64,
    runner_name: &'a str,
    scale_set_id: i64,
    policy_generation: u64,
    policy_digest: &'a str,
}

async fn request_and_bind<T>(
    lane: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
    request: JitRequestIdentity<'_>,
) -> Result<(RunnerReference, EncodedJit), EnsureError>
where
    T: Transport + Lane,
{
    let body = jit_request(request.runner_name).map_err(|_| EnsureError::Malformed)?;
    lane.on_admin().map_err(|_| EnsureError::Endpoint)?;
    let claim = scoped
        .claim_runner_jit_for_request(
            request.intent_id,
            request.reservation_id,
            request.request_id,
            request.runner_name,
            request.policy_generation,
            request.policy_digest,
        )
        .await
        .map_err(|_| EnsureError::Uncertain)?
        .ok_or(EnsureError::Uncertain)?;
    let result = match jit(lane, request.scale_set_id, admin_token, &body) {
        Ok(result) => result,
        Err(error) => {
            let failure = crate::listen::map_listen(error);
            if matches!(failure, EnsureError::Conflict) {
                scoped
                    .mark_runner_jit_conflict_received(
                        claim,
                        request.reservation_id,
                        request.policy_generation,
                        request.policy_digest,
                    )
                    .await
                    .map_err(|_| EnsureError::Uncertain)?;
            } else {
                scoped
                    .mark_runner_jit_uncertain(claim)
                    .await
                    .map_err(|_| EnsureError::Uncertain)?;
            }
            return Err(failure);
        }
    };
    if !runner_matches(&result.runner, request.scale_set_id, request.runner_name) {
        scoped
            .mark_runner_jit_uncertain(claim)
            .await
            .map_err(|_| EnsureError::Uncertain)?;
        return Err(EnsureError::Malformed);
    }
    scoped
        .bind_jit_runner(claim, &result.runner)
        .await
        .map_err(|_| EnsureError::Uncertain)?;
    Ok((result.runner, result.encoded_jit_config))
}

fn runner_matches(runner: &RunnerReference, scale_set_id: i64, requested_name: &str) -> bool {
    runner.id > 0 && runner.runner_scale_set_id == scale_set_id && runner.name == requested_name
}

impl JitReadyBootstrap {
    pub(crate) async fn measure_channel(
        self,
        docker: &Docker,
        engine: &crate::launch::SelectedEngineContext,
        images: &VerifiedWorkerImages,
        deadline: Instant,
    ) -> Result<ChannelReadyBootstrap, PendingChannelBootstrap> {
        let channel = measure_proof_channel(
            docker,
            ProofChannelMeasurement {
                engine,
                images,
                acquired: self.bound.prepared.acquired(),
                started: self.bound.started.receipt(),
                storage: &self.bound.binding,
                metadata: self.bound.prepared.metadata(),
                runner: &self.runner,
                deadline,
            },
        )
        .await;
        match channel {
            Ok(channel) => Ok(ChannelReadyBootstrap {
                bound: self.bound,

                jit: self.jit,
                channel,
            }),
            Err(failure) => {
                let _ = &failure;
                Err(PendingChannelBootstrap {
                    bound: Box::new(self.bound),
                    _runner: self.runner,
                    _jit: self.jit,
                })
            }
        }
    }
}

#[must_use]
pub(crate) struct PendingChannelBootstrap {
    bound: Box<BoundScopedBootstrap>,

    _runner: RunnerReference,
    _jit: EncodedJit,
}

impl PendingChannelBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }
}

impl ChannelReadyBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }

    pub(crate) fn channel(&self) -> &VerifiedProofChannelBinding {
        &self.channel
    }

    pub(super) async fn deliver(
        mut self,
        scoped: &ConnectionJournal,
        deadline: Instant,
    ) -> Result<RunnerReadyBootstrap, PendingJitBootstrap> {
        let reservation_id = self.bound.prepared.acquired().reservation_id();
        let Ok(Some(claim)) = scoped.claim_listener_delivery(reservation_id).await else {
            return Err(PendingJitBootstrap {
                bound: Box::new(self.bound),
            });
        };
        let Ok(proof) = self
            .bound
            .started
            .deliver_jit(&claim, self.jit, deadline)
            .await
        else {
            let _marked = scoped.mark_listener_delivery_uncertain(&claim).await;
            return Err(PendingJitBootstrap {
                bound: Box::new(self.bound),
            });
        };
        if scoped
            .record_listener_delivered(&claim, &proof)
            .await
            .is_err()
        {
            let _marked = scoped.mark_listener_delivery_uncertain(&claim).await;
            return Err(PendingJitBootstrap {
                bound: Box::new(self.bound),
            });
        }
        Ok(RunnerReadyBootstrap {
            bound: self.bound,

            channel: self.channel,
        })
    }
}
