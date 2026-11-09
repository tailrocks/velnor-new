// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]

pub mod probe_record;

mod docker;
mod docker_io;
mod error;
mod host_config;
mod journal;
mod mutation_cleanup;
mod mutation_lifecycle;
mod probe_image;
mod probes;
mod resource_dind;
mod resources;
mod tool_versions;

pub use docker::{Qualification, QualificationSession, open_qualification_session, qualify};
pub use error::QualificationError;
pub use probe_image::qualify as qualify_probe;
pub use probes::DaemonIdentity;
pub use tool_versions::VersionEvidence;

#[derive(Debug, Clone)]
pub struct SourceIdentity {
    pub source_sha: String,
    pub runner_version: String,
    pub docker_version: String,
    pub buildx_version: String,
    pub compose_version: String,
    pub node_version: String,
}

use std::future::Future;
use std::time::{Duration, Instant};

use bollard::errors::Error as DockerError;
use bollard::models::{Mount, MountType, Volume};
use bollard::query_parameters::CreateContainerOptions;

use crate::docker_io::{append, read_logs, wait_success};
use crate::host_config::validate_host_config;

use crate::error::QualificationError as HostError;

const QUALIFIER_LABEL: &str = "io.velnor.runner-image-qualifier.run";
const OUTPUT_LIMIT: usize = 4096;

async fn deadline<F: Future>(future: F, limit: Duration) -> Result<F::Output, HostError> {
    tokio::time::timeout(limit, future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

fn remaining(deadline: Instant) -> Result<Duration, HostError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(HostError::DockerTimeout)
    } else {
        Ok(remaining)
    }
}

fn create_options(name: &str) -> CreateContainerOptions {
    CreateContainerOptions {
        name: Some(name.to_owned()),
        platform: "linux/amd64".to_owned(),
    }
}

fn mount(volume: &str, target: &str) -> Mount {
    Mount {
        target: Some(target.to_owned()),
        source: Some(volume.to_owned()),
        typ: Some(MountType::VOLUME),
        read_only: Some(false),
        ..Default::default()
    }
}

fn volume_owned(volume: &Volume, name: &str, run_id: &str, role: &str) -> bool {
    volume.name == name
        && volume.labels.get(QUALIFIER_LABEL).map(String::as_str) == Some(run_id)
        && volume
            .labels
            .get("io.velnor.runner-image-qualifier.role")
            .map(String::as_str)
            == Some(role)
}

fn valid_container_id(value: &str) -> Result<(), HostError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn missing(error: &DockerError) -> bool {
    matches!(
        error,
        DockerError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

#[must_use]
pub fn same_healthcheck(
    left: Option<&bollard::models::HealthConfig>,
    right: Option<&bollard::models::HealthConfig>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.test == right.test
                && left.interval.unwrap_or_default() == right.interval.unwrap_or_default()
                && left.timeout.unwrap_or_default() == right.timeout.unwrap_or_default()
                && left.retries.unwrap_or_default() == right.retries.unwrap_or_default()
                && left.start_period.unwrap_or_default() == right.start_period.unwrap_or_default()
                && left.start_interval.unwrap_or_default()
                    == right.start_interval.unwrap_or_default()
        }
        _ => false,
    }
}
