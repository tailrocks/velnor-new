//! Bounded sampling and freshness cache for controller-owned guest probes.

mod adapter;
mod cache;
mod cleanup;
mod exit;
mod identity;
mod reconcile;
mod snapshot;

pub(crate) use cache::{GuestSampleCache, GuestSamplerTask, spawn_guest_sampler};
pub(crate) use identity::GuestProbeIdentity;
pub(crate) use snapshot::{GuestSampleFailure, GuestSampleSnapshot, GuestSampleStatus};

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bollard::models::SystemInfo;
use tokio::time::{Instant as TokioInstant, sleep, timeout};

use super::{GuestProbeCreate, GuestResourceSample, parse_sample, probe_container};
use identity::{
    IMAGE_REFERENCE, container_name, owner_labels, require_owned_id, trusted_image_id,
    valid_container_id,
};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_RUN_TIMEOUT: Duration = Duration::from_secs(5);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);
const CLEANUP_RESERVE: Duration = Duration::from_secs(6);
const CREATE_RECONCILE_TIMEOUT: Duration = Duration::from_secs(4);
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The small Docker API used by the sampler and its deterministic fake.
trait GuestDockerClient: Send + Sync {
    /// Read the selected daemon identity and guest capacity.
    fn info(&self) -> impl Future<Output = Result<SystemInfo, GuestSampleFailure>> + Send;

    /// Resolve only an image already present in the selected daemon.
    fn inspect_image(
        &self,
        reference: &str,
    ) -> impl Future<Output = Result<Option<ProbeImage>, GuestSampleFailure>> + Send;

    /// Inspect a probe by its controller-reserved name or known daemon ID.
    fn inspect_container(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<ProbeContainer>, GuestSampleFailure>> + Send;

    /// Create the fixed probe request and return its daemon-assigned ID.
    fn create_container(
        &self,
        request: GuestProbeCreate,
    ) -> impl Future<Output = Result<CreatedProbe, GuestSampleFailure>> + Send;

    /// Start an already inspected controller-owned probe.
    fn start_container(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<(), GuestSampleFailure>> + Send;

    /// Read the fixed sample file while the probe is alive.
    ///
    /// `None` means the selected daemon confirmed the file is not ready yet.
    fn read_output(
        &self,
        id: &str,
        limit: usize,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, GuestSampleFailure>> + Send;

    /// Force-remove a controller-owned probe ID.
    fn remove_container(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<(), GuestSampleFailure>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeImage {
    id: Option<String>,
    repo_tags: Vec<String>,
    os: Option<String>,
    architecture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeContainer {
    id: Option<String>,
    name: Option<String>,
    image: Option<String>,
    labels: HashMap<String, String>,
    running: Option<bool>,
    exit_code: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatedProbe {
    id: String,
    has_warnings: bool,
}

/// One bounded sample attempt.
struct GuestSampler<'a, C> {
    client: Arc<C>,
    identity: &'a GuestProbeIdentity,
    timing: SamplerTiming,
}

#[derive(Debug, Clone, Copy)]
struct SamplerTiming {
    operation: Duration,
    probe_run: Duration,
    cleanup: Duration,
    attempt: Duration,
    cleanup_reserve: Duration,
    create_reconcile: Duration,
    exit_poll: Duration,
}

struct SampleAttempt {
    snapshot: GuestSampleSnapshot,
    task_failure: Option<GuestSampleFailure>,
}

impl Default for SamplerTiming {
    fn default() -> Self {
        Self {
            operation: OPERATION_TIMEOUT,
            probe_run: PROBE_RUN_TIMEOUT,
            cleanup: CLEANUP_TIMEOUT,
            attempt: ATTEMPT_TIMEOUT,
            cleanup_reserve: CLEANUP_RESERVE,
            create_reconcile: CREATE_RECONCILE_TIMEOUT,
            exit_poll: EXIT_POLL_INTERVAL,
        }
    }
}

impl<'a, C: GuestDockerClient> GuestSampler<'a, C> {
    /// Bind the attempt to one selected Docker engine and durable worker volume.
    fn new(client: Arc<C>, identity: &'a GuestProbeIdentity) -> Self {
        Self {
            client,
            identity,
            timing: SamplerTiming::default(),
        }
    }

    fn with_timing(mut self, timing: SamplerTiming) -> Self {
        self.timing = timing;
        self
    }

    async fn call<T>(
        &self,
        deadline: TokioInstant,
        future: impl Future<Output = Result<T, GuestSampleFailure>>,
    ) -> Result<T, GuestSampleFailure> {
        let call_deadline = deadline.min(TokioInstant::now() + self.timing.operation);
        let remaining = call_deadline.saturating_duration_since(TokioInstant::now());
        let result = bounded_timeout(remaining, future).await?;
        if TokioInstant::now() >= call_deadline {
            Err(GuestSampleFailure::Timeout)
        } else {
            Ok(result)
        }
    }

    async fn cleanup_call<T>(
        &self,
        deadline: TokioInstant,
        future: impl Future<Output = Result<T, GuestSampleFailure>>,
    ) -> Result<T, GuestSampleFailure> {
        let call_deadline = deadline.min(TokioInstant::now() + self.timing.cleanup);
        let remaining = call_deadline.saturating_duration_since(TokioInstant::now());
        let result = bounded_timeout(remaining, future).await?;
        if TokioInstant::now() >= call_deadline {
            Err(GuestSampleFailure::Timeout)
        } else {
            Ok(result)
        }
    }

    /// Read, parse, and clean one probe attempt.
    async fn sample_once(&self) -> GuestSampleSnapshot {
        self.sample_attempt().await.snapshot
    }

    async fn sample_attempt(&self) -> SampleAttempt {
        let sampled_at = Instant::now();
        let attempt_deadline = TokioInstant::now() + self.timing.attempt;
        let cleanup_reserve = self.timing.cleanup_reserve.min(self.timing.attempt);
        let work_deadline = attempt_deadline - cleanup_reserve;
        let mut engine_verified = false;
        let mut owned_id = None;
        let mut expected_image = None;
        let result = self
            .collect_sample(
                work_deadline,
                &mut engine_verified,
                &mut owned_id,
                &mut expected_image,
            )
            .await;
        let create_uncertain = matches!(&result, Err(GuestSampleFailure::CreateUncertain));
        let cleanup = if engine_verified {
            self.cleanup_probe(
                owned_id.as_deref(),
                expected_image.as_deref(),
                create_uncertain,
                attempt_deadline,
            )
            .await
        } else {
            Ok(())
        };
        match cleanup {
            Ok(()) => SampleAttempt {
                snapshot: GuestSampleSnapshot::completed(result, sampled_at),
                task_failure: None,
            },
            Err(reason) => SampleAttempt {
                snapshot: GuestSampleSnapshot::completed(Err(reason), sampled_at),
                task_failure: Some(reason),
            },
        }
    }

    async fn collect_sample(
        &self,
        deadline: TokioInstant,
        engine_verified: &mut bool,
        owned_id: &mut Option<String>,
        expected_image: &mut Option<String>,
    ) -> Result<GuestResourceSample, GuestSampleFailure> {
        self.verify_engine(deadline).await?;
        *engine_verified = true;
        let name = container_name(self.identity);
        let labels = owner_labels(self.identity);

        if let Some(existing) = self
            .call(deadline, self.client.inspect_container(&name))
            .await?
        {
            *owned_id = Some(require_owned_id(&existing, &name, &labels)?);
            let id = owned_id.as_deref().ok_or(GuestSampleFailure::Ownership)?;
            self.remove_owned(id, &labels, None, deadline).await?;
            *owned_id = None;
        }

        let image = self
            .call(deadline, self.client.inspect_image(IMAGE_REFERENCE))
            .await?
            .ok_or(GuestSampleFailure::ProbeImage)?;
        let image_id = trusted_image_id(&image)?;
        *expected_image = Some(image_id.clone());
        let info = self.verify_engine(deadline).await?;
        let mut request = probe_container(&info).ok_or(GuestSampleFailure::ProbeInput)?;
        request.config.image = Some(image_id.clone());
        request.options.name = Some(name.clone());
        request.config.labels = Some(labels.clone());

        self.verify_engine(deadline).await?;
        match self
            .call(deadline, self.client.create_container(request))
            .await
        {
            Ok(created) => {
                if !valid_container_id(&created.id) {
                    return Err(GuestSampleFailure::ProbeCreate);
                }
                *owned_id = Some(created.id.clone());
                if created.has_warnings {
                    return Err(GuestSampleFailure::ProbeCreate);
                }
                self.start_and_read(deadline, &name, &labels, &image_id, &created.id, &info)
                    .await
            }
            Err(_) => {
                match self
                    .reconcile_create(&name, &labels, &image_id, deadline, owned_id)
                    .await
                {
                    Ok(()) => Err(GuestSampleFailure::ProbeCreate),
                    Err(GuestSampleFailure::EngineIdentity) => {
                        Err(GuestSampleFailure::EngineIdentity)
                    }
                    Err(_) => Err(GuestSampleFailure::CreateUncertain),
                }
            }
        }
    }

    async fn start_and_read(
        &self,
        deadline: TokioInstant,
        name: &str,
        labels: &HashMap<String, String>,
        image_id: &str,
        create_id: &str,
        info: &SystemInfo,
    ) -> Result<GuestResourceSample, GuestSampleFailure> {
        self.verify_engine(deadline).await?;
        let container = self
            .call(deadline, self.client.inspect_container(name))
            .await?
            .ok_or(GuestSampleFailure::ProbeCreate)?;
        let id = require_owned_id(&container, name, labels)?;
        if id != create_id || container.image.as_deref() != Some(image_id) {
            return Err(GuestSampleFailure::Ownership);
        }
        self.verify_engine(deadline).await?;
        self.call(deadline, self.client.start_container(&id))
            .await?;
        let probe_deadline = (TokioInstant::now() + self.timing.probe_run).min(deadline);
        let bytes = self.read_output_until_ready(&id, probe_deadline).await?;
        let exit_code = self
            .wait_for_exit(name, labels, &id, probe_deadline)
            .await?;
        if exit_code != 0 {
            return Err(GuestSampleFailure::ProbeExit);
        }
        let output = std::str::from_utf8(&bytes).map_err(|_| GuestSampleFailure::ProbeOutput)?;
        if output.is_empty() {
            return Err(GuestSampleFailure::ProbeOutput);
        }
        Ok(parse_sample(info, Some(output)))
    }

    async fn read_output_until_ready(
        &self,
        id: &str,
        deadline: TokioInstant,
    ) -> Result<Vec<u8>, GuestSampleFailure> {
        loop {
            if TokioInstant::now() >= deadline {
                return Err(GuestSampleFailure::Timeout);
            }
            self.verify_engine(deadline).await?;
            if let Some(bytes) = self
                .call(
                    deadline,
                    self.client.read_output(id, super::MAX_PROBE_OUTPUT_BYTES),
                )
                .await?
            {
                if bytes.len() > super::MAX_PROBE_OUTPUT_BYTES {
                    return Err(GuestSampleFailure::OutputLimit);
                }
                return Ok(bytes);
            }
            let remaining = deadline.saturating_duration_since(TokioInstant::now());
            if remaining.is_zero() {
                return Err(GuestSampleFailure::Timeout);
            }
            sleep(self.timing.exit_poll.min(remaining)).await;
        }
    }
}

async fn bounded_timeout<T>(
    limit: Duration,
    future: impl Future<Output = Result<T, GuestSampleFailure>>,
) -> Result<T, GuestSampleFailure> {
    if limit.is_zero() {
        return Err(GuestSampleFailure::Timeout);
    }
    timeout(limit, future)
        .await
        .map_err(|_| GuestSampleFailure::Timeout)?
}
