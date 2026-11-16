// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

use super::fixture::FixtureDocker;
use super::seed::{self, RESERVATION};
use super::{FAILED, RETIRED, TestResult, abandon_and_retire_unbound_pair, retire_unbound_pair};

#[tokio::test(flavor = "multi_thread")]
async fn a_started_pair_with_a_live_storage_initializer_is_retired_with_its_helper() -> TestResult<()>
{
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    fixture.add_initializer(FAILED);
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    seed::initializer(&journal, FAILED, "not_requested", "not_requested").await?;
    fixture.take_requests();

    assert!(abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);

    assert!(!fixture.has_initializer(FAILED));
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    let requests = fixture.take_requests();
    let helper = format!("DELETE /containers/{}", FAILED.helper_id());
    let removed = requests.iter().position(|request| *request == helper);
    let first_volume = requests
        .iter()
        .position(|request| request.starts_with("DELETE /volumes/"));
    assert!(removed.is_some() && removed < first_volume, "{requests:?}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_storage_initializer_that_is_not_the_pairs_helper_is_left_alone() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    fixture.add_initializer(FAILED);
    fixture.relabel_initializer(FAILED);
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    seed::initializer(&journal, FAILED, "not_requested", "not_requested").await?;

    let outcome =
        abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await;

    assert!(outcome.is_err());
    assert!(fixture.has_initializer(FAILED));
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "uncertain/00/uncertain/0/1/none/"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_the_initializer_is_removed_converges_on_restart() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    fixture.add_initializer(FAILED);
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    seed::initializer(&journal, FAILED, "not_requested", "not_requested").await?;

    fixture.hang_after(format!("DELETE /containers/{}", FAILED.helper_id()));
    tokio::select! {
        _ = Box::pin(abandon_and_retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation)) => {
            return Err("the retirement ended before the daemon was killed".into());
        }
        () = fixture.hung() => {}
    }
    assert!(!fixture.has_initializer(FAILED));
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "uncertain/00/uncertain/0/1/none/"
    );

    journal.recover_after_restart_without_authority().await?;
    assert!(retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    Ok(())
}
