//! Initial session statistics must not overfill or redundantly cover capacity.

use super::inspect_tests::{
    DockerStub, http, inspect_error, journal, launch_row, launch_row_for_id,
    no_response_body_in_journal, within,
};
use crate::IntentState;
use crate::launch::turn;

#[tokio::test]
async fn covered_spare_capacity_skips_the_initial_scale_callback() -> Result<(), String> {
    let (scratch, journal) = journal("initial-covered-population").await?;
    let row_id = launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, r#"{"State":{"Running":true}}"#)])?;
    let mut scaled = false;

    let result = within(
        turn::scale_if_free(&journal, &stub.docker, 2, 1, || async {
            scaled = true;
            Ok(None)
        }),
        "initial scale admission",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(result, Ok(None));
    assert!(!scaled);
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].id, row_id);
    assert_eq!(after[0].state, IntentState::Pending);
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn uncovered_spare_capacity_runs_the_initial_scale_callback() -> Result<(), String> {
    let (_scratch, journal) = journal("initial-uncovered-population").await?;
    launch_row(&journal).await?;
    let stub = DockerStub::open(vec![http(200, r#"{"State":{"Running":true}}"#)])?;
    let mut scaled = false;

    let result = within(
        turn::scale_if_free(&journal, &stub.docker, 2, 2, || async {
            scaled = true;
            Ok(None)
        }),
        "initial scale admission",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(result, Ok(None));
    assert!(scaled);
    Ok(())
}

#[tokio::test]
async fn full_capacity_skips_initial_scale_even_if_population_is_larger() -> Result<(), String> {
    let (scratch, journal) = journal("initial-full-capacity").await?;
    launch_row(&journal).await?;
    launch_row_for_id(&journal, "job-2", "runner-id-2").await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, r#"{"State":{"Running":true}}"#),
        http(200, r#"{"State":{"Running":true}}"#),
    ])?;
    let mut scaled = false;

    let result = within(
        turn::scale_if_free(&journal, &stub.docker, 2, 3, || async {
            scaled = true;
            Ok(None)
        }),
        "initial scale admission",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(result, Ok(None));
    assert!(!scaled);
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        before
    );
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn unknown_capacity_observation_does_not_run_the_scale_callback() -> Result<(), String> {
    let (scratch, journal) = journal("initial-unknown-capacity").await?;
    launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(500, r#"{"message":"temporary failure"}"#)])?;
    let mut scaled = false;

    let result = within(
        turn::scale_if_free(&journal, &stub.docker, 2, 1, || async {
            scaled = true;
            Ok(None)
        }),
        "initial scale admission",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(result, Err(inspect_error(500)));
    assert!(!scaled);
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        before
    );
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn nonpositive_population_skips_inspection_and_the_scale_callback() -> Result<(), String> {
    let (_scratch, journal) = journal("initial-nonpositive-population").await?;
    launch_row(&journal).await?;
    for population in [0, -1] {
        let stub = DockerStub::open(Vec::new())?;
        let mut scaled = false;
        let result = within(
            turn::scale_if_free(&journal, &stub.docker, 2, population, || async {
                scaled = true;
                Ok(None)
            }),
            "initial scale admission",
        )
        .await?;
        stub.finish().await?;

        assert_eq!(result, Ok(None));
        assert!(!scaled);
    }
    Ok(())
}

#[tokio::test]
async fn large_population_is_not_clamped_to_the_running_count_type() -> Result<(), String> {
    let (_scratch, journal) = journal("initial-large-population").await?;
    launch_row(&journal).await?;
    let stub = DockerStub::open(vec![http(200, r#"{"State":{"Running":true}}"#)])?;
    let mut scaled = false;

    let result = within(
        turn::scale_if_free(&journal, &stub.docker, 2, i64::MAX, || async {
            scaled = true;
            Ok(None)
        }),
        "initial scale admission",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(result, Ok(None));
    assert!(scaled);
    Ok(())
}

#[tokio::test]
async fn stopped_or_absent_worker_leaves_population_uncovered() -> Result<(), String> {
    let (_scratch, journal) = journal("initial-stopped-worker").await?;
    launch_row(&journal).await?;
    for response in [
        http(200, r#"{"State":{"Running":false}}"#),
        http(404, r#"{"message":"missing"}"#),
    ] {
        let stub = DockerStub::open(vec![response])?;
        let mut scaled = false;
        let result = within(
            turn::scale_if_free(&journal, &stub.docker, 2, 1, || async {
                scaled = true;
                Ok(None)
            }),
            "initial scale admission",
        )
        .await?;
        stub.finish().await?;

        assert_eq!(result, Ok(None));
        assert!(scaled);
    }
    Ok(())
}
