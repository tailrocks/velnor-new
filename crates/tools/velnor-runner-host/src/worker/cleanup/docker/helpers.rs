use std::future::Future;
use std::time::Duration;

use crate::HostError;

use super::super::{OuterContainerRole, WorkerGenerationIdentity};

const DIND_DOCKER_CLI: &str = "/usr/local/bin/docker";
pub(super) const CLEANUP_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn dind_exec_argv(arguments: &[&str]) -> Vec<String> {
    std::iter::once(DIND_DOCKER_CLI)
        .chain(arguments.iter().copied())
        .map(str::to_owned)
        .collect()
}

pub(super) async fn bounded_cleanup_operation<T>(
    timeout: Duration,
    future: impl Future<Output = Result<T, HostError>>,
) -> Result<T, HostError> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| HostError::Docker)?
}

pub(super) fn outer_id(identity: &WorkerGenerationIdentity, role: OuterContainerRole) -> &str {
    match role {
        OuterContainerRole::Runner => identity.runner_container_id(),
        OuterContainerRole::Dind => identity.dind_container_id(),
    }
}

pub(super) fn outer_name(identity: &WorkerGenerationIdentity, role: OuterContainerRole) -> String {
    format!("{}-{}", identity.worker_volume(), role_label(role))
}

pub(super) const fn role_label(role: OuterContainerRole) -> &'static str {
    match role {
        OuterContainerRole::Runner => "runner",
        OuterContainerRole::Dind => "dind",
    }
}

pub(super) fn parse_ids(output: &str) -> Result<Vec<String>, HostError> {
    let mut ids = Vec::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if !valid_docker_id(line) {
            return Err(HostError::Docker);
        }
        ids.push(line.to_owned());
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

pub(super) fn valid_docker_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::time::Duration;

    use futures_util::{StreamExt, stream};

    use super::{bounded_cleanup_operation, dind_exec_argv};
    use crate::HostError;

    #[test]
    fn private_daemon_exec_uses_the_image_docker_binary() {
        assert_eq!(
            dind_exec_argv(&["container", "ls", "--all", "--quiet"]),
            [
                "/usr/local/bin/docker",
                "container",
                "ls",
                "--all",
                "--quiet"
            ]
        );
        assert_eq!(
            dind_exec_argv(&["network", "rm", "a123456789ab"]),
            ["/usr/local/bin/docker", "network", "rm", "a123456789ab"]
        );
    }

    #[tokio::test]
    async fn whole_operation_deadline_bounds_silent_and_trickling_streams() {
        let duration = Duration::from_millis(25);
        let silent =
            bounded_cleanup_operation(duration, async { pending::<Result<(), HostError>>().await })
                .await;
        assert_eq!(silent, Err(HostError::Docker));

        let trickling = stream::unfold(0_u8, |count| async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            Some((Ok::<u8, HostError>(count), count.wrapping_add(1)))
        });
        let result = bounded_cleanup_operation(duration, async move {
            let mut trickling = std::pin::pin!(trickling);
            while let Some(item) = trickling.next().await {
                let _ = item?;
            }
            Ok(())
        })
        .await;
        assert_eq!(result, Err(HostError::Docker));
    }
}
