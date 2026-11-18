// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use velnor_runner_github::{
    Exchange, Method, SessionRequest, Transport, TransportFail,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use super::{Recording, create_and_verify};
use crate::{
    config::HostConfig,
    journal::{ExistingScaleSetObservation, Journal, ScaleSetCreateRequest, ScaleSetCreateState},
    scale_set::EnsureError,
};

const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

const REPOSITORY: &str =
    r#"{"id":42,"name":"repo","full_name":"owner/repo","owner":{"login":"owner"},"private":true}"#;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn scale_set_json(disable_update: bool) -> String {
    format!(
        r#"{{"id":10,"name":"ubuntu-26.04-scale-set","labels":[{{"name":"velnor"}},{{"name":"ubuntu-26.04-scale-set"}}],"runnerSetting":{{"disableUpdate":{disable_update}}}}}"#
    )
}

struct Script {
    replies: VecDeque<(u16, String)>,
    seen: Vec<(Method, String)>,
}

impl Script {
    fn new(replies: Vec<(u16, String)>) -> Self {
        Self {
            replies: VecDeque::from(replies),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push((request.method, request.path.clone()));
        let (status, body) = self.replies.pop_front().ok_or(TransportFail::Reset)?;
        Ok(Exchange {
            status,
            body: body.into_bytes(),
        })
    }
}

async fn run(
    dir: &tempfile::TempDir,
    replies: Vec<(u16, String)>,
) -> TestResult<(Result<i64, EnsureError>, Script, Journal)> {
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    let (outcome, transport) = attempt(&journal, replies).await?;
    Ok((outcome, transport, journal))
}

async fn attempt(
    journal: &Journal,
    replies: Vec<(u16, String)>,
) -> TestResult<(Result<i64, EnsureError>, Script)> {
    attempt_after(journal, replies, 0).await
}

async fn attempt_after(
    journal: &Journal,
    replies: Vec<(u16, String)>,
    earlier_status: u16,
) -> TestResult<(Result<i64, EnsureError>, Script)> {
    let config = HostConfig::parse(CONFIG)?;
    let binding = config.connections().first().ok_or("connection")?;
    let scoped = journal.connection_scope(binding).await?;
    let request = ScaleSetCreateRequest::new(binding, 1)?;
    let record = scoped.prepare_scale_set_create(&request).await?;
    let mut identity = Script::new(vec![(200, REPOSITORY.to_owned())]);
    let scope = get_repository_scope(
        &mut identity,
        RepositoryTarget {
            id: 42,
            owner: "owner",
            name: "repo",
        },
        "pat",
    )?;
    let mut transport = Recording::new(Script::new(replies));
    transport.status = earlier_status;
    let outcome = create_and_verify(
        "pat",
        &scope,
        &mut transport,
        "admin-token",
        &scoped,
        &request,
        &record,
    )
    .await
    .map(|set| set.id);
    Ok((outcome, transport.inner))
}

async fn bound_scale_set_ids(journal: &Journal) -> TestResult<Vec<i64>> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE scale_set_id IS NOT NULL",
            (),
        )
        .await?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await? {
        ids.push(row.get::<i64>(0)?);
    }
    Ok(ids)
}

async fn intent_states(journal: &Journal) -> TestResult<Vec<String>> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query("SELECT state FROM connection_scale_set_intents", ())
        .await?;
    let mut states = Vec::new();
    while let Some(row) = rows.next().await? {
        states.push(row.get::<String>(0)?);
    }
    Ok(states)
}

#[tokio::test]
async fn create_rereads_the_created_set_and_records_it() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, transport, journal) = run(
        &dir,
        vec![(200, scale_set_json(true)), (200, scale_set_json(true))],
    )
    .await?;
    assert_eq!(outcome, Ok(10));
    let methods: Vec<Method> = transport.seen.iter().map(|(method, _)| *method).collect();
    assert_eq!(methods, [Method::Post, Method::Get]);
    assert!(transport.seen[1].1.ends_with("/runnerscalesets/10"));
    assert_eq!(bound_scale_set_ids(&journal).await?, [10]);
    Ok(())
}

#[tokio::test]
async fn create_refuses_a_set_whose_reread_keeps_updates_enabled() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, transport, journal) = run(
        &dir,
        vec![(200, scale_set_json(true)), (200, scale_set_json(false))],
    )
    .await?;
    assert_eq!(transport.seen.len(), 2, "the created set must be re-read");
    assert!(outcome.is_err(), "outcome: {outcome:?}");
    assert_eq!(bound_scale_set_ids(&journal).await?, Vec::<i64>::new());
    assert_eq!(intent_states(&journal).await?, ["uncertain"]);
    Ok(())
}

#[tokio::test]
async fn create_refuses_a_post_body_that_keeps_updates_enabled() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, _transport, journal) = run(&dir, vec![(200, scale_set_json(false))]).await?;
    assert!(outcome.is_err(), "outcome: {outcome:?}");
    assert_eq!(bound_scale_set_ids(&journal).await?, Vec::<i64>::new());
    Ok(())
}

fn methods(transport: &Script) -> Vec<Method> {
    transport.seen.iter().map(|(method, _)| *method).collect()
}

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
async fn only_a_response_that_proves_a_refusal_releases_the_row() -> TestResult<()> {
    let released = [400, 401, 403, 404, 422];
    let held = [
        201, 202, 204, 299, 301, 302, 308, 408, 429, 500, 502, 503, 504,
    ];
    for status in released.into_iter().chain(held) {
        let dir = tempfile::tempdir()?;
        let (outcome, transport, journal) = run(&dir, vec![(status, String::new())]).await?;
        let expected = if released.contains(&status) {
            "prepared"
        } else {
            "uncertain"
        };
        assert_eq!(
            intent_states(&journal).await?,
            [expected],
            "status {status}: {outcome:?}"
        );
        assert_eq!(methods(&transport), [Method::Post], "status {status}");
        let (again, transport) = attempt(&journal, vec![(200, scale_set_json(true))]).await?;
        assert_eq!(
            transport.seen.is_empty(),
            expected == "uncertain",
            "status {status}: {again:?}"
        );
    }
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
