use std::time::Instant;

use crate::{config::RepositoryConnection, error::HostError};

use super::refresh_with_dependencies;
#[path = "refresh_tests/support.rs"]
mod support;
use support::on_large_test_stack;
#[path = "refresh_tests/fixture.rs"]
mod fixture;
use fixture::{
    BusyHostFixture, assert_current_ready, poll_receipt, prepare_busy_host,
    prepare_busy_host_without_sample_on_thread, probe_seed,
};

#[path = "refresh_tests/durable.rs"]
mod durable;

#[test]
fn controller_refresh_publishes_ready_for_a_healthy_busy_host() -> Result<(), String> {
    on_large_test_stack(controller_refresh_publishes_ready_for_a_healthy_busy_host_async)
}

async fn controller_refresh_publishes_ready_for_a_healthy_busy_host_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host()).await?;
    let revision_before_refresh = host.fixture.journal.revision().await.map_err(|error| {
        format!("The test could not read the journal revision before refresh. Error: {error}.")
    })?;
    let expected = host.fixture.runner.clone();
    let binding = host.binding.clone();
    let remote = move |actual_binding: &RepositoryConnection,
                       actual: Option<&velnor_runner_github::RunnerReference>,
                       set_id: i64| {
        if actual_binding == &binding && actual == Some(&expected) && set_id == 501 {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    };
    let refresh = Box::pin(refresh_with_dependencies(
        &host.state,
        host.docker.client(),
        &host.fixture.journal,
        &host.selected,
        &host.control,
        None,
        &remote,
    ))
    .await;
    let requests = host.docker.finish_observed().await.map_err(|error| {
        format!("The test could not collect the Docker request sequence. Error: {error}.")
    })?;
    refresh.map_err(|error| {
        format!("The controller refresh failed after requests {requests:?}. Error: {error}.")
    })?;
    assert_eq!(requests.len(), 2 + host.identity.volumes().len() + 1);
    assert_current_ready(
        &host.fixture.journal,
        &host.state,
        &host.selected,
        &host.control,
        revision_before_refresh,
    )
    .await
}

#[test]
fn controller_refresh_rejects_a_mismatched_remote_runner() -> Result<(), String> {
    on_large_test_stack(controller_refresh_rejects_a_mismatched_remote_runner_async)
}

async fn controller_refresh_rejects_a_mismatched_remote_runner_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host()).await?;
    let revision = host
        .fixture
        .journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    let reject = |_: &RepositoryConnection,
                  _: Option<&velnor_runner_github::RunnerReference>,
                  _: i64| Err(HostError::Ownership);
    let result = Box::pin(refresh_with_dependencies(
        &host.state,
        host.docker.client(),
        &host.fixture.journal,
        &host.selected,
        &host.control,
        None,
        &reject,
    ))
    .await;
    let requests = host.docker.finish_observed().await?;
    if !matches!(result, Err(HostError::Ownership)) {
        return Err(format!(
            "The controller accepted a mismatched remote runner: {result:?}."
        ));
    }
    assert_eq!(requests.len(), 2 + host.identity.volumes().len());
    let (_, config_sha256) = crate::readiness_scan::controller_config(&host.state)
        .ok_or_else(|| "The controller test configuration could not be read.".to_owned())?;
    if host
        .control
        .readiness_matches(
            &config_sha256,
            host.selected.engine_id(),
            host.selected.docker_root_digest(),
            &host
                .fixture
                .journal
                .instance_id()
                .await
                .map_err(|error| error.to_string())?,
            revision,
            Instant::now(),
        )
        .map_err(|error| error.to_string())?
    {
        return Err("The controller published Ready after a remote identity mismatch.".to_owned());
    }
    Ok(())
}

#[test]
fn controller_refresh_rejects_a_journal_revision_change_during_remote_audit() -> Result<(), String>
{
    on_large_test_stack(controller_refresh_rejects_a_journal_revision_change_async)
}

async fn controller_refresh_rejects_a_journal_revision_change_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host()).await?;
    let revision = host
        .fixture
        .journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    let scope = host.fixture.scope.clone();
    let engine_id = host.selected.engine_id().to_owned();
    let endpoint = host.selected.endpoint().to_owned();
    let platform = host.selected.platform().to_owned();
    let mutate = move |_: &RepositoryConnection,
                       _: Option<&velnor_runner_github::RunnerReference>,
                       _: i64| {
        let scope = scope.clone();
        let engine_id = engine_id.clone();
        let endpoint = endpoint.clone();
        let platform = platform.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| HostError::Journal)?;
            runtime.block_on(scope.bind_engine_context(&engine_id, &endpoint, &platform))
        })
        .join()
        .map_err(|_| HostError::Journal)?
    };
    Box::pin(refresh_with_dependencies(
        &host.state,
        host.docker.client(),
        &host.fixture.journal,
        &host.selected,
        &host.control,
        None,
        &mutate,
    ))
    .await
    .map_err(|error| error.to_string())?;
    let requests = host.docker.finish_observed().await?;
    assert_eq!(requests.len(), 2 + host.identity.volumes().len() + 1);
    let current_revision = host
        .fixture
        .journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    if current_revision <= revision {
        return Err("The concurrent journal update did not advance its revision.".to_owned());
    }
    if host
        .control
        .readiness_matches(
            &crate::readiness_scan::controller_config(&host.state)
                .ok_or_else(|| "The controller test configuration could not be read.".to_owned())?
                .1,
            host.selected.engine_id(),
            host.selected.docker_root_digest(),
            &host
                .fixture
                .journal
                .instance_id()
                .await
                .map_err(|error| error.to_string())?,
            current_revision,
            Instant::now(),
        )
        .map_err(|error| error.to_string())?
    {
        return Err("The controller published Ready after a journal revision change.".to_owned());
    }
    Ok(())
}
