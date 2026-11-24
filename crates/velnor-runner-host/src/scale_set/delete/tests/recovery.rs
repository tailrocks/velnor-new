// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::{
    Fixture, Reply, ScaleSetDeleteOutcome, Script, TestResult, delete_state, found, list, ok,
};
use crate::journal::{
    ConnectionJournal, Journal, ScaleSetCreateRequest, ScaleSetCreateState, ScaleSetDeleteGate,
};
use crate::scale_set::delete::{finish_recovered_delete, recover_intended_delete};
use crate::scale_set::{ScaleSetLookupAction, lookup_action};

async fn intend_and_crash(fixture: &Fixture) -> TestResult<ConnectionJournal> {
    let ScaleSetDeleteGate::Owned(target) = fixture.scoped.scale_set_delete_gate().await? else {
        return Err("gate".into());
    };
    let _crashed_claim = fixture.scoped.claim_scale_set_delete(&target).await?;
    assert_eq!(delete_state(fixture).await?.as_deref(), Some("requested"));
    Ok(fixture.journal.connection_scope(&fixture.binding).await?)
}

async fn recover(
    restarted: &ConnectionJournal,
    replies: Vec<Reply>,
) -> TestResult<(Option<ScaleSetDeleteOutcome>, Script)> {
    let mut script = Script::new(replies);
    let outcome = recover_intended_delete(&mut script, "admin-token", restarted).await?;
    Ok((outcome, script))
}

#[tokio::test]
async fn a_crash_after_the_intent_and_before_the_delete_retries_the_delete() -> TestResult<()> {
    let fixture = super::fixture(true).await?;
    let restarted = intend_and_crash(&fixture).await?;
    let (outcome, script) = recover(&restarted, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Deleted));
    assert_eq!(script.deletes(), 1);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

#[tokio::test]
async fn a_crash_after_the_delete_and_before_the_record_converges_on_the_404() -> TestResult<()> {
    let fixture = super::fixture(true).await?;
    let restarted = intend_and_crash(&fixture).await?;
    let (outcome, script) = recover(&restarted, vec![ok(404), list(&[])]).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Deleted));
    assert_eq!(script.deletes(), 0);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

#[tokio::test]
async fn a_404_without_an_intent_is_still_an_error() -> TestResult<()> {
    let fixture = super::fixture(true).await?;
    let (outcome, script) = attempt_recovery(&fixture).await?;
    assert_eq!(outcome, None);
    assert_eq!(script.requests(), 0);
    assert_eq!(delete_state(&fixture).await?, None);
    for organization in [false, true] {
        assert_eq!(
            lookup_action(ScaleSetCreateState::Created, false, organization),
            ScaleSetLookupAction::Hold
        );
    }
    Ok(())
}

async fn attempt_recovery(
    fixture: &Fixture,
) -> TestResult<(Option<ScaleSetDeleteOutcome>, Script)> {
    recover(&fixture.scoped, vec![ok(404), list(&[])]).await
}

struct Spy<'a> {
    script: Script,
    journal: &'a Journal,
    state_at_delete: Vec<Option<String>>,
}

fn committed_state(journal: &Journal) -> TestResult<Option<String>> {
    let read = std::thread::scope(|scope| {
        scope
            .spawn(|| -> Result<Option<String>, String> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .map_err(|error| error.to_string())?;
                runtime
                    .block_on(async {
                        let connection = journal.connection().await?;
                        let mut rows = connection
                            .query("SELECT state FROM connection_scale_set_deletes", ())
                            .await?;
                        Ok::<_, Box<dyn std::error::Error>>(match rows.next().await? {
                            Some(row) => Some(row.get::<String>(0)?),
                            None => None,
                        })
                    })
                    .map_err(|error| error.to_string())
            })
            .join()
    });
    read.map_err(|_| "reader panicked")?.map_err(Into::into)
}

impl Transport for Spy<'_> {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.method == Method::Delete {
            self.state_at_delete
                .push(committed_state(self.journal).map_err(|_| TransportFail::Reset)?);
        }
        self.script.exchange(request)
    }
}

#[tokio::test]
async fn the_intent_is_committed_before_the_delete_is_issued() -> TestResult<()> {
    let fixture = super::fixture(true).await?;
    let mut spy = Spy {
        script: Script::new(vec![found(), ok(204)]),
        journal: &fixture.journal,
        state_at_delete: Vec::new(),
    };
    let outcome = super::run_created_delete(&mut spy, "admin-token", &fixture.scoped).await?;
    assert_eq!(outcome, ScaleSetDeleteOutcome::Deleted);
    assert_eq!(spy.state_at_delete, vec![Some("requested".to_owned())]);
    assert_eq!(delete_state(&fixture).await?.as_deref(), Some("deleted"));
    Ok(())
}

async fn next_lookup(fixture: &Fixture) -> TestResult<ScaleSetLookupAction> {
    let request = ScaleSetCreateRequest::new(&fixture.binding, 1)?;
    let scoped = fixture.journal.connection_scope(&fixture.binding).await?;
    let record = scoped.prepare_scale_set_create(&request).await?;
    Ok(lookup_action(record.state(), false, false))
}

#[tokio::test]
async fn a_recovered_delete_lets_setup_create_a_new_set() -> TestResult<()> {
    let fixture = super::fixture(true).await?;
    let restarted = intend_and_crash(&fixture).await?;
    let (outcome, _) = recover(&restarted, vec![found(), ok(204)]).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Deleted));
    finish_recovered_delete(&fixture.journal, &fixture.binding, outcome).await?;
    assert_eq!(next_lookup(&fixture).await?, ScaleSetLookupAction::Create);
    assert_eq!(delete_state(&fixture).await?, None);
    Ok(())
}

#[tokio::test]
async fn an_uncertain_recovery_keeps_the_created_set_held() -> TestResult<()> {
    let fixture = super::fixture(true).await?;
    let restarted = intend_and_crash(&fixture).await?;
    let (outcome, _) = recover(&restarted, vec![ok(500)]).await?;
    assert_eq!(outcome, Some(ScaleSetDeleteOutcome::Uncertain));
    finish_recovered_delete(&fixture.journal, &fixture.binding, outcome).await?;
    assert_eq!(next_lookup(&fixture).await?, ScaleSetLookupAction::Hold);
    Ok(())
}
