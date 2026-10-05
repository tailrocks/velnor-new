//! Nonblocking sample cache and independent background refresh loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::sleep;

use crate::journal::LaunchIdentity;

use super::adapter::BollardGuestDocker;
use super::{GuestDockerClient, GuestSampleFailure, GuestSampler};
use super::{GuestSampleSnapshot, GuestSampleStatus};

/// Read-only access to the sampler's last completed result.
#[derive(Debug, Clone)]
pub(crate) struct GuestSampleCache {
    samples: watch::Receiver<GuestSampleSnapshot>,
    stop: watch::Sender<bool>,
}

impl GuestSampleCache {
    /// Return a fresh snapshot without waiting for Docker.
    #[must_use]
    pub(crate) fn latest(&self, max_age: Duration) -> GuestSampleSnapshot {
        (*self.samples.borrow()).with_freshness(max_age, Instant::now())
    }

    /// Ask the background sampler to stop after its current bounded probe.
    pub(crate) fn shutdown(&self) {
        let _ = self.stop.send_replace(true);
    }
}

/// Start a background sampler bound to the selected engine identity.
pub(crate) fn spawn_guest_sampler(
    docker: bollard::Docker,
    identity: LaunchIdentity,
    interval: Duration,
) -> Result<GuestSampleCache, GuestSampleFailure> {
    if interval.is_zero() {
        return Err(GuestSampleFailure::InvalidInterval);
    }
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| GuestSampleFailure::RuntimeUnavailable)?;
    let client = Arc::new(BollardGuestDocker::new(docker));
    let (sample_sender, samples) = watch::channel(GuestSampleSnapshot::pending());
    let (stop, stop_receiver) = watch::channel(false);
    runtime.spawn(sampling_loop(
        client,
        identity,
        interval,
        sample_sender,
        stop_receiver,
    ));
    Ok(GuestSampleCache { samples, stop })
}

async fn sampling_loop<C: GuestDockerClient + 'static>(
    client: Arc<C>,
    identity: LaunchIdentity,
    interval: Duration,
    samples: watch::Sender<GuestSampleSnapshot>,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        if *stop.borrow() {
            break;
        }
        let snapshot = GuestSampler::new(Arc::clone(&client), &identity)
            .sample_once()
            .await;
        if samples.send(snapshot).is_err() {
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
