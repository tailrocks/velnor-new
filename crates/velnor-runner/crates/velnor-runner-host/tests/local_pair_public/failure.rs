// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;
use velnor_runner_host::local::{StartFailure, start_pair};

use super::support::{
    Fixture, Program, TestResult, build_program, labelled, request, suffix, untag_only,
};

#[tokio::test]
async fn local_pair_start_refuses_a_runner_that_exits_at_once_and_cleans_up() -> TestResult<()> {
    let Some(fixture) = Fixture::new().await? else {
        return Ok(());
    };
    let tag = format!("velnor-local-test-exiter:{}", suffix()?);
    let built = build_program(&fixture.docker, &tag, Program::Exit).await;
    let failed = match &built {
        Ok(exiter) => {
            let mut pair = fixture.request();
            pair.runner_image.clone_from(exiter);
            Some(Box::pin(start_pair(&fixture.docker, &pair)).await)
        }
        Err(_) => None,
    };
    let left = labelled(&fixture.docker, &fixture.environment).await;
    let removed = fixture
        .docker
        .remove_image(&tag, Some(untag_only()), None)
        .await;
    fixture.finish().await?;
    built?;
    removed?;
    assert_eq!(failed, Some(Err(StartFailure::from(HostError::Ownership))));
    assert_eq!(left?, (0, 0));
    Ok(())
}

#[tokio::test]
async fn local_pair_start_failure_names_the_pair_when_cleanup_cannot_run() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket)?);
    let docker = velnor_runner_host::connect_unix(&socket.to_string_lossy())?;
    let image = format!("sha256:{}", "a".repeat(64));
    let failure = Box::pin(start_pair(
        &docker,
        &request("env-one", "velnor-local-net", &image),
    ))
    .await
    .err()
    .ok_or("start succeeded")?;
    assert_eq!(failure.error, HostError::Docker);
    assert!(failure.leftover.is_some_and(|name| !name.is_empty()));
    Ok(())
}
