//! Nonblocking sample cache and independent background refresh loop.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::sleep;

use super::adapter::BollardGuestDocker;
use super::{
    GuestDockerClient, GuestProbeIdentity, GuestSampleFailure, GuestSampler, SamplerTiming,
};
use super::{GuestSampleSnapshot, GuestSampleStatus};

/// Read-only access to the sampler's last completed result.
#[derive(Debug, Clone)]
pub(crate) struct GuestSampleCache {
    samples: watch::Receiver<GuestSampleSnapshot>,
    task_failure: watch::Receiver<Option<GuestSampleFailure>>,
}

impl GuestSampleCache {
    /// Return a fresh snapshot without waiting for Docker.
    #[must_use]
    pub(crate) fn latest(&self, max_age: Duration) -> GuestSampleSnapshot {
        if let Some(reason) = *self.task_failure.borrow() {
            return GuestSampleSnapshot::completed(Err(reason), Instant::now());
        }
        (*self.samples.borrow()).with_freshness(max_age, Instant::now())
    }
}

/// Owned task handle. Call [`shutdown`](Self::shutdown) before runtime shutdown.
#[derive(Debug)]
#[must_use = "retain the sampler task and await shutdown before stopping the host runtime"]
pub(crate) struct GuestSamplerTask {
    stop: watch::Sender<bool>,
    task_failure: watch::Sender<Option<GuestSampleFailure>>,
    thread: Option<JoinHandle<Result<(), GuestSampleFailure>>>,
}

impl GuestSamplerTask {
    /// Stop refreshes, await bounded probe cleanup, and join the sampler thread.
    pub(crate) async fn shutdown(mut self) -> Result<(), GuestSampleFailure> {
        self.request_stop();
        let thread = self.thread.take().ok_or(GuestSampleFailure::SamplerTask)?;
        let result = match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let task_failure = self.task_failure.clone();
                match runtime
                    .spawn_blocking(move || {
                        let result = join_sampler_thread(thread);
                        if let Err(reason) = result {
                            drop(task_failure.send_replace(Some(reason)));
                        }
                        result
                    })
                    .await
                {
                    Ok(result) => result,
                    Err(_) => Err(GuestSampleFailure::SamplerTask),
                }
            }
            Err(_) => join_sampler_thread(thread),
        };
        if let Err(reason) = result {
            self.publish_failure(reason);
        }
        result
    }

    fn request_stop(&self) {
        self.stop.send_replace(true);
    }

    fn publish_failure(&self, reason: GuestSampleFailure) {
        drop(self.task_failure.send_replace(Some(reason)));
    }
}

impl Drop for GuestSamplerTask {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(thread) = self.thread.take() {
            if let Err(reason) = join_sampler_thread(thread) {
                self.publish_failure(reason);
            }
        }
    }
}

fn join_sampler_thread(
    thread: JoinHandle<Result<(), GuestSampleFailure>>,
) -> Result<(), GuestSampleFailure> {
    match thread.join() {
        Ok(result) => result,
        Err(_) => Err(GuestSampleFailure::SamplerTask),
    }
}

/// Start an independent sampler bound to one worker and selected engine.
///
/// The caller owns the returned task and must await its shutdown before the
/// host runtime ends. Cache reads never wait for probe I/O.
pub(crate) fn spawn_guest_sampler(
    docker: bollard::Docker,
    identity: GuestProbeIdentity,
    interval: Duration,
) -> Result<(GuestSampleCache, GuestSamplerTask), GuestSampleFailure> {
    let client = Arc::new(BollardGuestDocker::new(docker));
    spawn_sampling_thread(client, identity, interval, SamplerTiming::default())
}

pub(super) fn spawn_sampling_thread<C: GuestDockerClient + 'static>(
    client: Arc<C>,
    identity: GuestProbeIdentity,
    interval: Duration,
    timing: SamplerTiming,
) -> Result<(GuestSampleCache, GuestSamplerTask), GuestSampleFailure> {
    if interval.is_zero() {
        return Err(GuestSampleFailure::InvalidInterval);
    }
    let (sample_sender, samples) = watch::channel(GuestSampleSnapshot::pending());
    let (stop, stop_receiver) = watch::channel(false);
    let (task_failure, task_failures) = watch::channel(None);
    let thread = thread::Builder::new()
        .name("velnor-guest-resource-sampler".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| GuestSampleFailure::RuntimeUnavailable)?;
            runtime.block_on(sampling_loop(
                client,
                identity,
                interval,
                timing,
                sample_sender,
                stop_receiver,
            ))
        })
        .map_err(|_| GuestSampleFailure::RuntimeUnavailable)?;
    Ok((
        GuestSampleCache {
            samples,
            task_failure: task_failures,
        },
        GuestSamplerTask {
            stop,
            task_failure,
            thread: Some(thread),
        },
    ))
}

/// Run refreshes until shutdown, completing the current bounded attempt first.
pub(super) async fn sampling_loop<C: GuestDockerClient + 'static>(
    client: Arc<C>,
    identity: GuestProbeIdentity,
    interval: Duration,
    timing: SamplerTiming,
    samples: watch::Sender<GuestSampleSnapshot>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), GuestSampleFailure> {
    loop {
        if *stop.borrow() {
            break;
        }
        let attempt = GuestSampler::new(Arc::clone(&client), &identity)
            .with_timing(timing)
            .sample_attempt()
            .await;
        let sample_receiver_closed = samples.send(attempt.snapshot).is_err();
        if let Some(reason) = attempt.task_failure {
            return Err(reason);
        }
        if sample_receiver_closed {
            break;
        }
        tokio::select! {
            () = sleep(interval) => {}
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
        }
    }
    Ok(())
}

impl GuestSampleSnapshot {
    fn pending() -> Self {
        Self {
            sample: super::super::GuestResourceSample::default(),
            status: GuestSampleStatus::Pending,
            sampled_at: None,
        }
    }

    fn with_freshness(self, max_age: Duration, now: Instant) -> Self {
        let Some(sampled_at) = self.sampled_at else {
            return self;
        };
        if now.saturating_duration_since(sampled_at) <= max_age {
            return self;
        }
        Self {
            sample: super::super::GuestResourceSample::default(),
            status: GuestSampleStatus::Stale,
            sampled_at: Some(sampled_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{GuestSampleSnapshot, GuestSampleStatus};
    use crate::worker::resources::guest::GuestResourceSample;

    #[test]
    fn stale_samples_are_returned_as_unknown() {
        let collected = Instant::now();
        let sample = GuestSampleSnapshot {
            sample: GuestResourceSample {
                cpu_millicores: Some(4000),
                ..GuestResourceSample::default()
            },
            status: GuestSampleStatus::Available,
            sampled_at: Some(collected),
        };

        let result =
            sample.with_freshness(Duration::from_millis(1), collected + Duration::from_secs(1));

        assert_eq!(result.status, GuestSampleStatus::Stale);
        assert_eq!(result.sample, GuestResourceSample::default());
    }
}
