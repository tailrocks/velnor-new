use std::pin::pin;

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::query_parameters::DownloadFromContainerOptionsBuilder;
use futures_util::StreamExt;
use zeroize::Zeroizing;

use crate::HostError;
use crate::docker_client::docker_deadline;

use super::helpers::{CLEANUP_OPERATION_TIMEOUT, bounded_cleanup_operation, dind_exec_argv};
use super::{DockerCleanupEngine, OuterContainerRole, WorkerGenerationIdentity};

const MAX_EXEC_OUTPUT: usize = 1024 * 1024;
const MAX_RUNNER_DIAGNOSTICS: usize = 64 * 1024 * 1024;

impl DockerCleanupEngine<'_> {
    pub(super) async fn download_runner_diagnostics(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, HostError> {
        bounded_cleanup_operation(CLEANUP_OPERATION_TIMEOUT, async {
            let runner = self
                .inspect_owned(identity, OuterContainerRole::Runner)
                .await?;
            if !runner.present {
                return Ok(None);
            }
            if runner.running {
                return Err(HostError::Docker);
            }
            let options = DownloadFromContainerOptionsBuilder::default()
                .path("/home/runner/_diag")
                .build();
            let stream = self
                .docker
                .download_from_container(identity.runner_container_id(), Some(options));
            let mut stream = pin!(stream);
            let mut archive = Zeroizing::new(Vec::new());
            while let Some(chunk) = stream.as_mut().next().await {
                let chunk = chunk.map_err(|_| HostError::Docker)?;
                if archive.len().saturating_add(chunk.len()) > MAX_RUNNER_DIAGNOSTICS {
                    return Err(HostError::Frame);
                }
                archive.extend_from_slice(&chunk);
            }
            Ok(Some(archive))
        })
        .await
    }

    pub(super) async fn run_dind_command(
        &self,
        identity: &WorkerGenerationIdentity,
        command: Vec<&str>,
    ) -> Result<String, HostError> {
        bounded_cleanup_operation(CLEANUP_OPERATION_TIMEOUT, async {
            let dind = self
                .inspect_owned(identity, OuterContainerRole::Dind)
                .await?;
            if !dind.present || !dind.running {
                return Err(HostError::Docker);
            }
            let options = CreateExecOptions::<String> {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(dind_exec_argv(&command)),
                user: Some("0:0".to_owned()),
                ..Default::default()
            };
            let created = docker_deadline(
                self.docker
                    .create_exec(identity.dind_container_id(), options),
            )
            .await?
            .map_err(|_| HostError::Docker)?;
            if created.id.is_empty() {
                return Err(HostError::Docker);
            }
            let start = docker_deadline(self.docker.start_exec(
                &created.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: Some(16 * 1024),
                }),
            ))
            .await?
            .map_err(|_| HostError::Docker)?;
            let StartExecResults::Attached { mut output, .. } = start else {
                return Err(HostError::Docker);
            };
            let mut bytes = Zeroizing::new(Vec::new());
            while let Some(line) = output.next().await {
                let line = line.map_err(|_| HostError::Docker)?;
                let line = match line {
                    LogOutput::StdOut { message }
                    | LogOutput::StdErr { message }
                    | LogOutput::Console { message } => message,
                    LogOutput::StdIn { .. } => continue,
                };
                if bytes.len().saturating_add(line.len()) > MAX_EXEC_OUTPUT {
                    return Err(HostError::Frame);
                }
                bytes.extend_from_slice(&line);
            }
            let inspected = docker_deadline(self.docker.inspect_exec(&created.id))
                .await?
                .map_err(|_| HostError::Docker)?;
            if inspected.exit_code != Some(0) {
                return Err(HostError::Docker);
            }
            String::from_utf8(bytes.to_vec()).map_err(|_| HostError::Docker)
        })
        .await
    }
}
