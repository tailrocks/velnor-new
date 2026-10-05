//! Bounded sampling and freshness cache for controller-owned guest probes.

mod adapter;
mod cache;
mod identity;
mod snapshot;

pub(crate) use cache::{GuestSampleCache, spawn_guest_sampler};
pub(crate) use snapshot::{GuestSampleFailure, GuestSampleSnapshot, GuestSampleStatus};

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bollard::models::SystemInfo;
use tokio::time::{sleep, timeout};

use crate::journal::LaunchIdentity;

use super::{GuestProbeCreate, GuestResourceSample, parse_sample, probe_container};
use identity::{
    IMAGE_REFERENCE, container_name, owner_labels, require_owned_id, trusted_image_id,
    valid_container_id,
};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_RUN_TIMEOUT: Duration = Duration::from_secs(5);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
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

    /// Inspect a probe by its controller-reserved name.
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

    /// Read probe stdout without retaining more than `limit` bytes.
    fn read_output(
        &self,
        id: &str,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<u8>, GuestSampleFailure>> + Send;

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
    identity: &'a LaunchIdentity,
    timing: SamplerTiming,
}

#[derive(Debug, Clone, Copy)]
struct SamplerTiming {
    operation: Duration,
    probe_run: Duration,
    cleanup: Duration,
    exit_poll: Duration,
}

impl Default for SamplerTiming {
    fn default() -> Self {
        Self {
            operation: OPERATION_TIMEOUT,
            probe_run: PROBE_RUN_TIMEOUT,
            cleanup: CLEANUP_TIMEOUT,
            exit_poll: EXIT_POLL_INTERVAL,
        }
    }
}

impl<'a, C: GuestDockerClient> GuestSampler<'a, C> {
    /// Bind the attempt to one Docker client and launch identity.
    fn new(client: Arc<C>, identity: &'a LaunchIdentity) -> Self {
        Self {
            client,
            identity,
            timing: SamplerTiming::default(),
        }
    }

    #[cfg(test)]
    fn with_timing(mut self, timing: SamplerTiming) -> Self {
        self.timing = timing;
        self
    }

    async fn call<T>(
        &self,
        future: impl Future<Output = Result<T, GuestSampleFailure>>,
    ) -> Result<T, GuestSampleFailure> {
        bounded_timeout(self.timing.operation, future).await
    }

    async fn cleanup_call<T>(
        &self,
        future: impl Future<Output = Result<T, GuestSampleFailure>>,
    ) -> Result<T, GuestSampleFailure> {
        bounded_timeout(self.timing.cleanup, future).await
    }

    /// Read, parse, and clean one probe attempt.
    async fn sample_once(&self) -> GuestSampleSnapshot {
        let mut engine_verified = false;
        let mut owned_id = None;
        let result = self
            .collect_sample(&mut engine_verified, &mut owned_id)
            .await;
        let cleanup = if engine_verified {
            self.cleanup_probe(owned_id.as_deref()).await
        } else {
            Ok(())
        };
        GuestSampleSnapshot::completed(match cleanup {
            Ok(()) => result,
            Err(reason) => Err(reason),
        })
    }

    async fn collect_sample(
        &self,
        engine_verified: &mut bool,
        owned_id: &mut Option<String>,
    ) -> Result<GuestResourceSample, GuestSampleFailure> {
        self.verify_engine().await?;
        *engine_verified = true;

        let image = self
            .call(self.client.inspect_image(IMAGE_REFERENCE))
            .await?
            .ok_or(GuestSampleFailure::ProbeImage)?;
        let image_id = trusted_image_id(&image)?;
        let info = self.verify_engine().await?;
        let mut request = probe_container(&info).ok_or(GuestSampleFailure::ProbeInput)?;
        request.config.image = Some(image_id.clone());
        let name = container_name(self.identity);
        let labels = owner_labels(self.identity);
        request.options.name = Some(name.clone());
        request.config.labels = Some(labels.clone());

        if let Some(existing) = self.call(self.client.inspect_container(&name)).await? {
            *owned_id = Some(require_owned_id(&existing, &name, &labels)?);
            self.remove_owned(owned_id.as_deref().ok_or(GuestSampleFailure::Ownership)?)
                .await?;
            *owned_id = None;
        }

        self.verify_engine().await?;
        match self.call(self.client.create_container(request)).await {
            Ok(created) => {
                if !valid_container_id(&created.id) {
                    return Err(GuestSampleFailure::ProbeCreate);
                }
                *owned_id = Some(created.id.clone());
                if created.has_warnings {
                    return Err(GuestSampleFailure::ProbeCreate);
                }
                self.start_and_read(&name, &labels, &image_id, &created.id, &info)
                    .await
            }
            Err(error) => {
                self.verify_engine().await?;
                match self.call(self.client.inspect_container(&name)).await {
                    Ok(Some(container)) => {
                        *owned_id = Some(require_owned_id(&container, &name, &labels)?);
                        Err(GuestSampleFailure::ProbeCreate)
                    }
                    Ok(None) => Err(if error == GuestSampleFailure::Timeout {
                        GuestSampleFailure::ProbeCreate
                    } else {
                        error
                    }),
                    Err(_) => Err(GuestSampleFailure::ProbeCreate),
                }
            }
        }
    }

    async fn start_and_read(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
        image_id: &str,
        create_id: &str,
        info: &SystemInfo,
    ) -> Result<GuestResourceSample, GuestSampleFailure> {
        self.verify_engine().await?;
        let container = self
            .call(self.client.inspect_container(name))
            .await?
            .ok_or(GuestSampleFailure::ProbeCreate)?;
        let id = require_owned_id(&container, name, labels)?;
        if id != create_id || container.image.as_deref() != Some(image_id) {
            return Err(GuestSampleFailure::Ownership);
        }
        self.verify_engine().await?;
        self.call(self.client.start_container(&id)).await?;
        let exit_code = self.wait_for_exit(name, labels).await?;
        if exit_code != 0 {
            return Err(GuestSampleFailure::ProbeExit);
        }
        self.verify_engine().await?;
        let bytes = self
            .call(self.client.read_output(&id, super::MAX_PROBE_OUTPUT_BYTES))
            .await?;
        if bytes.len() > super::MAX_PROBE_OUTPUT_BYTES {
            return Err(GuestSampleFailure::OutputLimit);
        }
        let output = std::str::from_utf8(&bytes).map_err(|_| GuestSampleFailure::ProbeOutput)?;
        if output.is_empty() {
            return Err(GuestSampleFailure::ProbeOutput);
        }
        Ok(parse_sample(info, Some(output)))
    }

    async fn wait_for_exit(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
    ) -> Result<i64, GuestSampleFailure> {
        let deadline = Instant::now() + self.timing.probe_run;
        loop {
            if Instant::now() >= deadline {
                return Err(GuestSampleFailure::Timeout);
            }
            let container = self
                .call(self.client.inspect_container(name))
                .await?
                .ok_or(GuestSampleFailure::ProbeExit)?;
            require_owned_id(&container, name, labels)?;
            match (container.running, container.exit_code) {
                (Some(false), Some(code)) => return Ok(code),
                (Some(true), _) => sleep(self.timing.exit_poll).await,
                _ => return Err(GuestSampleFailure::ProbeExit),
            }
        }
    }

    async fn cleanup_probe(&self, expected_id: Option<&str>) -> Result<(), GuestSampleFailure> {
        self.verify_engine().await?;
        let name = container_name(self.identity);
        let labels = owner_labels(self.identity);
        match self
            .cleanup_call(self.client.inspect_container(&name))
            .await?
        {
            None => Ok(()),
            Some(container) => {
                let id = require_owned_id(&container, &name, &labels)?;
                if expected_id.is_some_and(|expected| expected != id) {
                    return Err(GuestSampleFailure::Ownership);
                }
                self.remove_owned(&id).await
            }
        }
    }

    async fn remove_owned(&self, id: &str) -> Result<(), GuestSampleFailure> {
        self.verify_engine().await?;
        // Container inspection reconciles both success and lost remove responses.
        match self.cleanup_call(self.client.remove_container(id)).await {
            Ok(()) | Err(_) => {}
        }
        let name = container_name(self.identity);
        let labels = owner_labels(self.identity);
        if self.inspect_after_remove(&name, &labels, id).await? {
            return Ok(());
        }
        self.verify_engine().await?;
        // A lost delete response is reconciled against the exact owned name and labels.
        match self.cleanup_call(self.client.remove_container(id)).await {
            Ok(()) | Err(_) => {}
        }
        if self.inspect_after_remove(&name, &labels, id).await? {
            Ok(())
        } else {
            Err(GuestSampleFailure::Cleanup)
        }
    }

    async fn inspect_after_remove(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
        id: &str,
    ) -> Result<bool, GuestSampleFailure> {
        self.verify_engine().await?;
        match self
            .cleanup_call(self.client.inspect_container(name))
            .await?
        {
            None => Ok(true),
            Some(container) if require_owned_id(&container, name, labels)? == id => Ok(false),
            Some(_) => Err(GuestSampleFailure::Ownership),
        }
    }

    async fn verify_engine(&self) -> Result<SystemInfo, GuestSampleFailure> {
        let info = self.call(self.client.info()).await?;
        if info.id.as_deref() == Some(self.identity.engine_id()) {
            Ok(info)
        } else {
            Err(GuestSampleFailure::EngineIdentity)
        }
    }
}

async fn bounded_timeout<T>(
    limit: Duration,
    future: impl Future<Output = Result<T, GuestSampleFailure>>,
) -> Result<T, GuestSampleFailure> {
    timeout(limit, future)
        .await
        .map_err(|_| GuestSampleFailure::Timeout)?
}
