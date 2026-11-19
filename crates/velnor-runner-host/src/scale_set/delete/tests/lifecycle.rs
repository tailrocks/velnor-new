// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::{
    CONFIG, Fixture, ScaleSetDeleteOutcome, TestResult, attempt, delete_state, fixture, found, ok,
    set_json,
};
use crate::config::HostConfig;
use crate::journal::{
    CreatedScaleSetResponse, Journal, ScaleSetCreateRequest, ScaleSetCreateState,
    ScaleSetDeleteGate,
};
use crate::scale_set::delete::recover_intended_delete;
use crate::scale_set::{ScaleSetLookupAction, lookup_action};

const NAME: &str = "ubuntu-26.04-scale-set";

struct FakeScaleSetApi {
    sets: BTreeSet<i64>,
    deletes: Vec<i64>,
}

impl FakeScaleSetApi {
    fn with(sets: &[i64]) -> Self {
        Self {
            sets: sets.iter().copied().collect(),
            deletes: Vec::new(),
        }
    }

    fn reply(status: u16, body: String) -> Exchange {
        Exchange {
            status,
            body: body.into_bytes(),
        }
    }
}

impl Transport for FakeScaleSetApi {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let id = request
            .path
            .rsplit('/')
            .next()
            .and_then(|tail| tail.parse::<i64>().ok());
        Ok(match (request.method, id) {
            (Method::Get, Some(id)) if self.sets.contains(&id) => {
                Self::reply(200, set_json(id, NAME))
            }
            (Method::Get, Some(_)) => Self::reply(404, String::new()),
            (Method::Get, None) => {
                let sets: Vec<String> = self.sets.iter().map(|id| set_json(*id, NAME)).collect();
                Self::reply(
                    200,
                    format!(r#"{{"count":{},"value":[{}]}}"#, sets.len(), sets.join(",")),
                )
            }
            (Method::Delete, Some(id)) => {
                self.deletes.push(id);
                if self.sets.remove(&id) {
                    Self::reply(204, String::new())
                } else {
                    Self::reply(404, String::new())
                }
            }
            _ => return Err(TransportFail::Reset),
        })
    }
}

struct Refused {
    _dir: tempfile::TempDir,
    journal: Journal,
    scoped: crate::journal::ConnectionJournal,
    request: ScaleSetCreateRequest,
}

async fn refused_create(retained: i64) -> TestResult<Refused> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    let config = HostConfig::parse(CONFIG)?;
    let binding = config.connections().first().ok_or("connection")?.clone();
    let scoped = journal.connection_scope(&binding).await?;
    let request = ScaleSetCreateRequest::new(&binding, 1)?;
    let record = scoped.prepare_scale_set_create(&request).await?;
    let claim = scoped
        .claim_scale_set_create(&record)
        .await?
        .ok_or("claim")?;
    let uncertain = scoped
        .mark_scale_set_create_uncertain(claim, Some(retained))
        .await?;
    assert_eq!(uncertain.state(), ScaleSetCreateState::Uncertain);
    Ok(Refused {
        _dir: dir,
        journal,
        scoped,
        request,
    })
}

async fn count(journal: &Journal, table: &str) -> TestResult<i64> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(&format!("SELECT COUNT(*) FROM {table}"), ())
        .await?;
    Ok(rows.next().await?.ok_or("count")?.get::<i64>(0)?)
}

async fn assert_create_can_proceed(refused: &Refused) -> TestResult<()> {
    let record = refused
        .scoped
        .prepare_scale_set_create(&refused.request)
        .await?;
    assert_eq!(record.state(), ScaleSetCreateState::Prepared);
    assert_eq!(
        lookup_action(record.state(), false, false),
        ScaleSetLookupAction::Create
    );
    assert!(
        refused
            .scoped
            .claim_scale_set_create(&record)
            .await?
            .is_some()
    );
    assert_eq!(
        count(&refused.journal, "connection_scale_set_unverified").await?,
        0
    );
    assert_eq!(
        count(&refused.journal, "connection_scale_set_deletes").await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn a_refused_create_deletes_the_remote_set_it_left_behind() -> TestResult<()> {
    let refused = refused_create(10).await?;
    let mut api = FakeScaleSetApi::with(&[10, 77]);
    let outcome = recover_intended_delete(&mut api, "admin-token", &refused.scoped).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Deleted));
    assert_eq!(api.deletes, vec![10]);
    assert_eq!(api.sets, BTreeSet::from([77]));
    assert_create_can_proceed(&refused).await
}

#[tokio::test]
async fn a_refused_create_whose_set_is_already_gone_sends_no_delete() -> TestResult<()> {
    let refused = refused_create(10).await?;
    let mut api = FakeScaleSetApi::with(&[]);
    let outcome = recover_intended_delete(&mut api, "admin-token", &refused.scoped).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Deleted));
    assert!(api.deletes.is_empty());
    assert_create_can_proceed(&refused).await
}

#[tokio::test]
async fn an_unclear_answer_keeps_the_refused_create_held() -> TestResult<()> {
    let refused = refused_create(10).await?;
    let mut script = super::Script::new(vec![ok(500)]);
    let outcome = recover_intended_delete(&mut script, "admin-token", &refused.scoped).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Uncertain));
    let record = refused
        .scoped
        .prepare_scale_set_create(&refused.request)
        .await?;
    assert_eq!(record.state(), ScaleSetCreateState::Uncertain);
    let mut api = FakeScaleSetApi::with(&[10]);
    let outcome = recover_intended_delete(&mut api, "admin-token", &refused.scoped).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Deleted));
    assert_eq!(api.deletes, vec![10]);
    assert_create_can_proceed(&refused).await
}

async fn disconnect_with_delete(fixture: &Fixture) -> TestResult<()> {
    let (outcome, _) = attempt(fixture, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::Deleted);
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn reconnecting_after_the_created_set_was_deleted_creates_a_new_set() -> TestResult<()> {
    let fixture = fixture(true).await?;
    disconnect_with_delete(&fixture).await?;
    assert!(fixture.journal.reactivate_if_drained(&fixture.binding).await?);
    let scoped = fixture.journal.connection_scope(&fixture.binding).await?;
    let request = ScaleSetCreateRequest::new(&fixture.binding, 1)?;
    let record = scoped.prepare_scale_set_create(&request).await?;
    assert_eq!(record.state(), ScaleSetCreateState::Prepared);
    assert_eq!(
        lookup_action(record.state(), false, false),
        ScaleSetLookupAction::Create
    );
    let claim = scoped
        .claim_scale_set_create(&record)
        .await?
        .ok_or("claim")?;
    let labels = ["velnor".to_owned(), NAME.to_owned()];
    let response = CreatedScaleSetResponse::verified(&request, 11, NAME, &labels, true)?;
    let created = scoped.record_scale_set_created(claim, response).await?;
    assert_eq!(created.receipt().map(|receipt| receipt.id), Some(11));
    assert_eq!(delete_state(&fixture).await?, None);
    let ScaleSetDeleteGate::Owned(target) = scoped.scale_set_delete_gate().await? else {
        return Err("the new set is not a fresh delete target".into());
    };
    assert_eq!((target.scale_set_id, target.state), (11, None));
    Ok(())
}

#[tokio::test]
async fn reactivating_without_a_set_delete_keeps_the_created_set() -> TestResult<()> {
    let fixture = fixture(true).await?;
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    assert!(fixture.journal.reactivate_if_drained(&fixture.binding).await?);
    let scoped = fixture.journal.connection_scope(&fixture.binding).await?;
    let request = ScaleSetCreateRequest::new(&fixture.binding, 1)?;
    let record = scoped.prepare_scale_set_create(&request).await?;
    assert_eq!(record.state(), ScaleSetCreateState::Created);
    assert_eq!(record.receipt().map(|receipt| receipt.id), Some(10));
    Ok(())
}
