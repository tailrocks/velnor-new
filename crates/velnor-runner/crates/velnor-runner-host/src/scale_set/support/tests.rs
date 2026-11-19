// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use velnor_runner_github::{
    Exchange, Method, SessionRequest, Transport, TransportFail,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use super::{CreateIds, Recording, create_and_verify};
use crate::{
    config::HostConfig,
    journal::{Journal, ScaleSetCreateRequest},
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

fn foreign_group_json() -> String {
    scale_set_json(true).replacen("{\"id\":10,", "{\"id\":10,\"runnerGroupId\":2,", 1)
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
    run_with(dir, replies, true).await
}

async fn run_with(
    dir: &tempfile::TempDir,
    replies: Vec<(u16, String)>,
    adopt_on_conflict: bool,
) -> TestResult<(Result<i64, EnsureError>, Script, Journal)> {
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
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
    let ids = CreateIds {
        pat: "pat",
        scope: &scope,
        adopt_on_conflict,
    };
    let outcome = create_and_verify(
        ids,
        &mut transport,
        "admin-token",
        &scoped,
        &request,
        &record,
    )
    .await
    .map(|set| set.id);
    Ok((outcome, transport.inner, journal))
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

#[tokio::test]
async fn create_refuses_a_set_from_a_foreign_runner_group() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, transport, journal) = run(&dir, vec![(200, foreign_group_json())]).await?;
    assert_eq!(outcome, Err(EnsureError::Rejected));
    assert_eq!(transport.seen.len(), 1, "a rejected set is not re-read");
    assert_eq!(bound_scale_set_ids(&journal).await?, Vec::<i64>::new());
    assert_eq!(intent_states(&journal).await?, ["uncertain"]);
    Ok(())
}

#[tokio::test]
async fn conflict_lookup_refuses_to_adopt_a_foreign_runner_group_set() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let page = format!(r#"{{"count":1,"value":[{}]}}"#, foreign_group_json());
    let (outcome, transport, journal) = run(&dir, vec![(409, String::new()), (200, page)]).await?;
    assert_eq!(outcome, Err(EnsureError::Rejected));
    let methods: Vec<Method> = transport.seen.iter().map(|(method, _)| *method).collect();
    assert_eq!(methods, [Method::Post, Method::Get]);
    assert_eq!(bound_scale_set_ids(&journal).await?, Vec::<i64>::new());
    Ok(())
}

#[tokio::test]
async fn an_organization_create_conflict_never_adopts_the_existing_set() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, transport, journal) = run_with(
        &dir,
        vec![(409, "{}".to_owned()), (200, scale_set_json(true))],
        false,
    )
    .await?;
    assert_eq!(outcome, Err(EnsureError::Uncertain));
    assert_eq!(transport.seen.len(), 1, "no lookup after the conflict");
    assert_eq!(bound_scale_set_ids(&journal).await?, Vec::<i64>::new());
    Ok(())
}
