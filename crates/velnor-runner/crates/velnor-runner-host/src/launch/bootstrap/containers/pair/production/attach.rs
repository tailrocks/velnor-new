use std::time::Instant;

use bollard::Docker;
use bollard::container::AttachContainerResults;
use bollard::query_parameters::AttachContainerOptionsBuilder;

use crate::error::HostError;

use super::verify::request;

pub(super) async fn attach_runner(
    docker: &Docker,
    runner_id: &str,
    deadline: Instant,
) -> Result<AttachContainerResults, HostError> {
    request(
        deadline,
        docker.attach_container(
            runner_id,
            Some(
                AttachContainerOptionsBuilder::default()
                    .stdin(true)
                    .stdout(false)
                    .stderr(false)
                    .stream(true)
                    .logs(false)
                    .build(),
            ),
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)
}
