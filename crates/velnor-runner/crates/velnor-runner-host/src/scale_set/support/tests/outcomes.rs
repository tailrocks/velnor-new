// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::Method;

use super::{
    CONFIG, EnsureError, ExistingScaleSetObservation, HostConfig, Journal, ScaleSetCreateRequest,
    ScaleSetCreateState, TestResult, attempt, attempt_after, bound_scale_set_ids, intent_states,
    methods, run, scale_set_json,
};

#[tokio::test]
async fn a_refused_create_is_forbidden_and_can_be_posted_again() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, transport, journal) = run(&dir, vec![(403, String::new())]).await?;
    assert_eq!(outcome, Err(EnsureError::Forbidden));
    assert_eq!(methods(&transport), [Method::Post]);
    assert_eq!(intent_states(&journal).await?, ["prepared"]);

    let (outcome, transport) = attempt(
        &journal,
        vec![(200, scale_set_json(true)), (200, scale_set_json(true))],
    )
    .await?;
    assert_eq!(outcome, Ok(10));
    assert_eq!(methods(&transport), [Method::Post, Method::Get]);
    Ok(())
}

#[tokio::test]
async fn other_definite_refusals_are_not_uncertain() -> TestResult<()> {
    for status in [400, 401, 404, 422] {
        let dir = tempfile::tempdir()?;
        let (outcome, _transport, journal) = run(&dir, vec![(status, String::new())]).await?;
        assert_eq!(
            outcome,
            Err(EnsureError::Unexpected {
                status,
                step: "runnerscalesets"
            })
        );
        assert_eq!(intent_states(&journal).await?, ["prepared"]);
    }
    Ok(())
}

#[tokio::test]
async fn a_server_error_create_stays_uncertain() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, _transport, journal) = run(&dir, vec![(500, String::new())]).await?;
    assert!(outcome.is_err(), "outcome: {outcome:?}");
    assert_eq!(intent_states(&journal).await?, ["uncertain"]);
    let (outcome, transport) = attempt(&journal, vec![(200, scale_set_json(true))]).await?;
    assert_eq!(outcome, Err(EnsureError::Uncertain));
    assert!(transport.seen.is_empty(), "an uncertain row must not post");
    Ok(())
}

#[tokio::test]
async fn a_conflict_whose_set_is_gone_can_be_posted_again() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let empty = r#"{"count":0,"value":[]}"#.to_owned();
    let (outcome, transport, journal) = run(&dir, vec![(409, String::new()), (200, empty)]).await?;
    assert_eq!(outcome, Err(EnsureError::Uncertain));
    assert_eq!(methods(&transport), [Method::Post, Method::Get]);
    assert_eq!(intent_states(&journal).await?, ["no_effect"]);

    let (outcome, transport) = attempt(
        &journal,
        vec![(200, scale_set_json(true)), (200, scale_set_json(true))],
    )
    .await?;
    assert_eq!(outcome, Ok(10));
    assert_eq!(methods(&transport), [Method::Post, Method::Get]);
    Ok(())
}

#[tokio::test]
async fn a_transport_failure_stays_uncertain() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, _transport, journal) = run(&dir, Vec::new()).await?;
    assert_eq!(outcome, Err(EnsureError::Uncertain));
    assert_eq!(intent_states(&journal).await?, ["uncertain"]);
    Ok(())
}

#[tokio::test]
async fn a_reset_does_not_reuse_an_earlier_refusal_status() -> TestResult<()> {
    for earlier in [403, 404, 409] {
        let dir = tempfile::tempdir()?;
        let journal = Journal::open(&dir.path().join("launch.db")).await?;
        journal.bind_engine("engine-test").await?;
        let (outcome, _transport) = attempt_after(&journal, Vec::new(), earlier).await?;
        assert_eq!(outcome, Err(EnsureError::Uncertain), "earlier {earlier}");
        assert_eq!(
            intent_states(&journal).await?,
            ["uncertain"],
            "earlier {earlier}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_malformed_success_body_stays_uncertain() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, _transport, journal) = run(&dir, vec![(200, "{".to_owned())]).await?;
    assert!(outcome.is_err(), "outcome: {outcome:?}");
    assert_eq!(intent_states(&journal).await?, ["uncertain"]);
    Ok(())
}

#[tokio::test]
async fn a_released_row_adopts_a_set_that_appears_later() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, _transport, journal) = run(&dir, vec![(403, String::new())]).await?;
    assert_eq!(outcome, Err(EnsureError::Forbidden));
    assert_eq!(intent_states(&journal).await?, ["prepared"]);

    let config = HostConfig::parse(CONFIG)?;
    let binding = config.connections().first().ok_or("connection")?;
    let scoped = journal.connection_scope(binding).await?;
    let request = ScaleSetCreateRequest::new(binding, 1)?;
    let labels = ["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()];
    let observed = ExistingScaleSetObservation::verified(
        &request,
        10,
        "ubuntu-26.04-scale-set",
        &labels,
        true,
    )?;
    let adopted = scoped.adopt_scale_set(&request, observed).await?;
    assert_eq!(adopted.state(), ScaleSetCreateState::Adopted);
    assert_eq!(intent_states(&journal).await?, ["adopted"]);
    assert_eq!(bound_scale_set_ids(&journal).await?, [10]);
    Ok(())
}
