//! Fail-closed Docker observations used by admission and reconciliation.

use bollard::Docker;

use velnor_runner_host::EnsureError;
use velnor_runner_host::docker_client::{classify_inspect, docker_deadline, inspect_error};

pub(crate) async fn container_running(docker: &Docker, id: &str) -> Result<bool, EnsureError> {
    let response = docker_deadline(docker.inspect_container(id, None))
        .await
        .map_err(|_| inspect_error(0))?;
    classify_inspect(response)
}

#[cfg(test)]
mod tests;
