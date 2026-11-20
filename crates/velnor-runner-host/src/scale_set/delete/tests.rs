// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::{ScaleSetDeleteOutcome, run_created_delete};
use crate::{
    config::{HostConfig, RepositoryConnection},
    journal::{
        ConnectionJournal, CreatedScaleSetResponse, ExistingScaleSetObservation, Journal,
        ScaleSetCreateRequest,
    },
};

pub(crate) const CONFIG: &str = r#"schema = 2
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

#[path = "tests/lifecycle.rs"]
mod lifecycle;
#[path = "tests/live_session.rs"]
mod live_session;
#[path = "tests/recovery.rs"]
mod recovery;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone)]
enum Reply {
    Status(u16, String),
    Reset,
}

fn ok(status: u16) -> Reply {
    Reply::Status(status, String::new())
}

fn set_json(id: i64, name: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","labels":[{{"name":"velnor"}},{{"name":"{name}"}}],"runnerSetting":{{"disableUpdate":true}}}}"#
    )
}

fn found() -> Reply {
    Reply::Status(200, set_json(10, "ubuntu-26.04-scale-set"))
}

fn list(sets: &[String]) -> Reply {
    Reply::Status(
        200,
        format!(r#"{{"count":{},"value":[{}]}}"#, sets.len(), sets.join(",")),
    )
}

struct Script {
    replies: VecDeque<Reply>,
    seen: Vec<(Method, String)>,
}

impl Script {
    fn new(replies: Vec<Reply>) -> Self {
        Self {
            replies: VecDeque::from(replies),
            seen: Vec::new(),
        }
    }

    fn requests(&self) -> usize {
        self.seen.len()
    }

    fn deletes(&self) -> usize {
        self.seen
            .iter()
            .filter(|(method, _)| *method == Method::Delete)
            .count()
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push((request.method, request.path.clone()));
        match self.replies.pop_front().unwrap_or(Reply::Reset) {
            Reply::Status(status, body) => Ok(Exchange {
                status,
                body: body.into_bytes(),
            }),
            Reply::Reset => Err(TransportFail::Reset),
        }
    }
}

pub(crate) struct Fixture {
    _dir: tempfile::TempDir,
    pub(crate) journal: Journal,
    pub(crate) binding: RepositoryConnection,
    pub(crate) scoped: ConnectionJournal,
}

pub(crate) async fn fixture(created: bool) -> TestResult<Fixture> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    let config = HostConfig::parse(CONFIG)?;
    let binding = config.connections().first().ok_or("connection")?.clone();
    let scoped = journal.connection_scope(&binding).await?;
    let request = ScaleSetCreateRequest::new(&binding, 1)?;
    let labels = ["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()];
    let record = scoped.prepare_scale_set_create(&request).await?;
    if created {
        let claim = scoped
            .claim_scale_set_create(&record)
            .await?
            .ok_or("claim")?;
        let response = CreatedScaleSetResponse::verified(
            &request,
            10,
            "ubuntu-26.04-scale-set",
            &labels,
            true,
        )?;
        scoped.record_scale_set_created(claim, response).await?;
    } else {
        let observed = ExistingScaleSetObservation::verified(
            &request,
            10,
            "ubuntu-26.04-scale-set",
            &labels,
            true,
        )?;
        scoped.adopt_scale_set(&request, observed).await?;
    }
    Ok(Fixture {
        _dir: dir,
        journal,
        binding,
        scoped,
    })
}

async fn attempt(
    fixture: &Fixture,
    replies: Vec<Reply>,
) -> TestResult<(ScaleSetDeleteOutcome, Script)> {
    let mut script = Script::new(replies);
    let outcome = run_created_delete(&mut script, "admin-token", &fixture.scoped).await?;
    Ok((outcome, script))
}

async fn delete_state(fixture: &Fixture) -> TestResult<Option<String>> {
    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query("SELECT state FROM connection_scale_set_deletes", ())
        .await?;
    Ok(match rows.next().await? {
        Some(row) => Some(row.get::<String>(0)?),
        None => None,
    })
}

fn is_delete_of_ten(entry: &(Method, String)) -> bool {
    entry.0 == Method::Delete && entry.1.ends_with("/runnerscalesets/10")
}

#[tokio::test]
async fn created_set_is_deleted_once_and_the_row_records_it() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let (outcome, script) = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(script.deletes(), 1);
    assert!(script.seen.iter().any(is_delete_of_ten));
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    let (replay, script) = attempt(&fixture, vec![]).await?;
    assert_eq!(replay, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(script.requests(), 0);
    Ok(())
}

#[tokio::test]
async fn adopted_set_is_never_deleted() -> TestResult<()> {
    let fixture = fixture(false).await?;
    let (outcome, script) = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::NotOwned);
    assert_eq!(script.requests(), 0);
    assert_eq!(delete_state(&fixture).await?, None);
    Ok(())
}

#[tokio::test]
async fn a_binding_that_disagrees_with_the_intent_sends_nothing() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "UPDATE connection_runtime_bindings SET set_ownership = 'adopted'",
            (),
        )
        .await?;
    let mut script = Script::new(vec![found(), ok(204)]);
    let outcome = run_created_delete(&mut script, "admin-token", &fixture.scoped).await;
    assert!(outcome.is_err(), "outcome: {outcome:?}");
    assert_eq!(script.requests(), 0);
    Ok(())
}

#[tokio::test]
async fn definite_refusals_leave_the_delete_ready_to_retry() -> TestResult<()> {
    let fixture = fixture(true).await?;
    for status in [400_u16, 401, 403, 409, 422] {
        let (outcome, script) = attempt(&fixture, vec![found(), ok(status)]).await?;
        assert_eq!(outcome, ScaleSetDeleteOutcome::Refused(status));
        assert_eq!(script.deletes(), 1);
        assert_eq!(delete_state(&fixture).await?.as_deref(), Some("prepared"));
    }
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn unclear_answers_are_uncertain_and_block_retirement() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let answers = [
        ok(500),
        ok(502),
        ok(408),
        ok(429),
        ok(201),
        ok(302),
        Reply::Reset,
    ];
    for answer in answers {
        let (outcome, script) = attempt(&fixture, vec![found(), answer]).await?;
        assert_eq!(outcome, ScaleSetDeleteOutcome::Uncertain);
        assert_eq!(script.deletes(), 1);
        assert_eq!(delete_state(&fixture).await?.as_deref(), Some("uncertain"));
        assert!(
            !fixture
                .journal
                .retire_connection_if_drained(&fixture.binding)
                .await?
        );
    }
    Ok(())
}

#[tokio::test]
async fn deleted_row_allows_retirement() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let _ = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn a_bare_404_is_trusted_only_when_the_group_list_proves_absence() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let ours = set_json(10, "ubuntu-26.04-scale-set");
    let unproven = [
        vec![found(), ok(404), ok(404), list(std::slice::from_ref(&ours))],
        vec![found(), ok(404), ok(404), ok(500)],
        vec![found(), ok(404), found()],
        vec![found(), ok(404), ok(403)],
    ];
    for replies in unproven {
        let (outcome, script) = attempt(&fixture, replies).await?;
        assert_eq!(outcome, ScaleSetDeleteOutcome::Uncertain);
        assert_eq!(script.deletes(), 1);
        assert_eq!(delete_state(&fixture).await?.as_deref(), Some("uncertain"));
    }
    let (outcome, _) = attempt(&fixture, vec![found(), ok(404), ok(404), list(&[])]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

#[tokio::test]
async fn a_set_that_no_longer_matches_the_journal_is_not_deleted() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let other = Reply::Status(200, set_json(10, "someone-elses-set"));
    let (outcome, script) = attempt(&fixture, vec![other, ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::Uncertain);
    assert_eq!(script.deletes(), 0);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("uncertain"));
    Ok(())
}

#[tokio::test]
async fn a_retry_after_uncertainty_observes_before_it_deletes_again() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let (first, _) = attempt(&fixture, vec![found(), ok(500)]).await?;
    assert_eq!(first, ScaleSetDeleteOutcome::Uncertain);
    let (second, script) = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert_eq!(second, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(script.deletes(), 1);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

#[tokio::test]
async fn a_retry_that_proves_absence_converges_without_a_delete() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let (first, _) = attempt(&fixture, vec![found(), ok(500)]).await?;
    assert_eq!(first, ScaleSetDeleteOutcome::Uncertain);
    let (second, script) = attempt(&fixture, vec![ok(404), list(&[])]).await?;
    assert_eq!(second, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(script.deletes(), 0);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

#[tokio::test]
async fn a_scope_with_pending_work_is_not_drained_and_sends_nothing() -> TestResult<()> {
    let fixture = fixture(true).await?;
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "INSERT INTO connection_session_creates (connection_id, scale_set_id, local_nonce, state) VALUES ('conn-test-1', 10, '0123456789abcdef0123456789abcdef', 'prepared')",
            (),
        )
        .await?;
    let (outcome, script) = attempt(&fixture, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::NotDrained);
    assert_eq!(script.requests(), 0);
    Ok(())
}

#[tokio::test]
async fn a_retired_scope_cannot_delete() -> TestResult<()> {
    let fixture = fixture(true).await?;
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    let mut script = Script::new(vec![found(), ok(204)]);
    let outcome = run_created_delete(&mut script, "admin-token", &fixture.scoped).await;
    assert!(outcome.is_err(), "outcome: {outcome:?}");
    assert_eq!(script.requests(), 0);
    Ok(())
}
