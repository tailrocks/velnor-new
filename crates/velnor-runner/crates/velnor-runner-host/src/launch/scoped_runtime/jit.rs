use bollard::Docker;
use std::time::Instant;
use velnor_runner_github::{EncodedJit, RunnerReference, Transport, jit, jit_request};

use crate::{journal::ConnectionJournal, launch::Lane, scale_set::EnsureError};

use super::BoundScopedBootstrap;
use crate::launch::VerifiedWorkerImages;
use crate::worker::execution_authorization::{VerifiedProofChannelBinding, measure_proof_channel};

#[cfg(test)]
#[path = "jit/acquired_resume_tests.rs"]
mod acquired_resume_tests;
#[cfg(test)]
#[path = "jit/adverse_effects_tests.rs"]
mod adverse_effects_tests;
#[cfg(test)]
#[path = "jit/tests.rs"]
mod tests;

/// Keep a bound runner and its one-time JIT value before delivery.
#[must_use]
pub(crate) struct JitReadyBootstrap {
    bound: BoundScopedBootstrap,
    runner: RunnerReference,
    jit: EncodedJit,
}

/// Keep the measured channel and one-time JIT value before publication.
#[must_use]
pub(crate) struct ChannelReadyBootstrap {
    bound: BoundScopedBootstrap,
    runner: RunnerReference,
    jit: EncodedJit,
    channel: VerifiedProofChannelBinding,
}

impl JitReadyBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }
}

/// Keep a bound runner when JIT creation or journal binding fails.
#[must_use]
pub(crate) struct PendingJitBootstrap {
    bound: BoundScopedBootstrap,
    failure: EnsureError,
}

/// Keep the delivered runner and its bound storage evidence.
#[must_use]
pub(crate) struct RunnerReadyBootstrap {
    bound: BoundScopedBootstrap,
    runner: RunnerReference,
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

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn channel(&self) -> &VerifiedProofChannelBinding {
        &self.channel
    }
}

impl PendingJitBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }

    pub(crate) const fn failure(&self) -> EnsureError {
        self.failure
    }
}

/// Claim one JIT operation, create the remote runner, and bind its exact identity.
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
            bound,
            failure: EnsureError::Uncertain,
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
        intent_id,
        reservation_id,
        request_id,
        &runner_name,
        scale_set_id,
        policy_generation,
        &policy_digest,
    )
    .await
    {
        Ok(result) => result,
        Err(failure) => return Err(PendingJitBootstrap { bound, failure }),
    };
    Ok(JitReadyBootstrap {
        bound,
        runner: result.0,
        jit: result.1,
    })
}

/// Claim and bind one JIT operation for a journal-bound physical intent.
async fn request_and_bind<T>(
    lane: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
    intent_id: i64,
    reservation_id: i64,
    request_id: i64,
    runner_name: &str,
    scale_set_id: i64,
    policy_generation: u64,
    policy_digest: &str,
) -> Result<(RunnerReference, EncodedJit), EnsureError>
where
    T: Transport + Lane,
{
    let body = jit_request(runner_name).map_err(|_| EnsureError::Malformed)?;
    lane.on_admin().map_err(|_| EnsureError::Endpoint)?;
    let claim = scoped
        .claim_runner_jit_for_request(
            intent_id,
            reservation_id,
            request_id,
            runner_name,
            policy_generation,
            policy_digest,
        )
        .await
        .map_err(|_| EnsureError::Uncertain)?
        .ok_or(EnsureError::Uncertain)?;
    let result = match jit(lane, scale_set_id, admin_token, &body) {
        Ok(result) => result,
        Err(error) => {
            let failure = crate::listen::map_listen(error);
            scoped
                .mark_runner_jit_uncertain(claim)
                .await
                .map_err(|_| EnsureError::Uncertain)?;
            return Err(failure);
        }
    };
    if !runner_matches(&result.runner, scale_set_id, runner_name) {
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
            engine,
            images,
            self.bound.prepared.acquired(),
            self.bound.started.receipt(),
            &self.bound.binding,
            self.bound.prepared.metadata(),
            &self.runner,
            deadline,
        )
        .await;
        match channel {
            Ok(channel) => Ok(ChannelReadyBootstrap {
                bound: self.bound,
                runner: self.runner,
                jit: self.jit,
                channel,
            }),
            Err(failure) => Err(PendingChannelBootstrap {
                bound: self.bound,
                runner: self.runner,
                jit: self.jit,
                failure,
            }),
        }
    }
}

/// Keep live runner handles and the JIT value when channel measurement fails.
#[must_use]
pub(crate) struct PendingChannelBootstrap {
    bound: BoundScopedBootstrap,
    runner: RunnerReference,
    jit: EncodedJit,
    failure: crate::error::HostError,
}

impl PendingChannelBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn failure(&self) -> &crate::error::HostError {
        &self.failure
    }
}

impl ChannelReadyBootstrap {
    pub(crate) fn bound(&self) -> &BoundScopedBootstrap {
        &self.bound
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
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
        let claim = match scoped.claim_listener_delivery(reservation_id).await {
            Ok(Some(claim)) => claim,
            Ok(None) | Err(_) => {
                return Err(PendingJitBootstrap {
                    bound: self.bound,
                    failure: EnsureError::Uncertain,
                });
            }
        };
        let proof = if let Ok(proof) = self
            .bound
            .started
            .deliver_jit(&claim, self.jit, deadline)
            .await
        {
            proof
        } else {
            let _marked = scoped.mark_listener_delivery_uncertain(&claim).await;
            return Err(PendingJitBootstrap {
                bound: self.bound,
                failure: EnsureError::Uncertain,
            });
        };
        if scoped
            .record_listener_delivered(&claim, &proof)
            .await
            .is_err()
        {
            let _marked = scoped.mark_listener_delivery_uncertain(&claim).await;
            return Err(PendingJitBootstrap {
                bound: self.bound,
                failure: EnsureError::Uncertain,
            });
        }
        Ok(RunnerReadyBootstrap {
            bound: self.bound,
            runner: self.runner,
            channel: self.channel,
        })
    }
}
