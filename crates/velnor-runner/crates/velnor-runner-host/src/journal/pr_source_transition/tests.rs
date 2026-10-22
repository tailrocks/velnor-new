use serde_json::Value;

#[path = "tests/plan_validation.rs"]
mod plan_validation;
use velnor_runner_github::workflow_run::{
    BaseRefChangedEvent, GitCommit, GitCommitIdentity, GitCommitParent, GitCommitPerson,
    GitCommitTree, PullRequestBaseHistory,
};

use super::ledger::TransitionLedger;
use super::plan::{TransitionPlan, TransitionPlanInput};
use super::receipt::{ReferenceReceipt, RetargetHistoryReceipt, RetargetResponseReceipt};
use super::types::{ResponseDisposition, TransitionError, TransitionStage, UncertaintyReason};

const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TARGET_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const COMMIT_SHA: &str = "dddddddddddddddddddddddddddddddddddddddd";
const TREE_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const EVENT_TIME: &str = "2025-01-02T03:04:05Z";

pub(in crate::journal::pr_source_transition) fn plan() -> Result<TransitionPlan, TransitionError> {
    let identity = GitCommitIdentity::try_new("Configured Person", "person@example.com")
        .map_err(|_| TransitionError::IncorrectPlan)?;
    TransitionPlan::try_new(TransitionPlanInput {
        connection_id: "sea".to_owned(),
        scale_set_id: 9,
        binding_digest: "1".repeat(64),
        repository_id: 41,
        repository_owner: "ChainArgos".to_owned(),
        repository_name: "java-monorepo".to_owned(),
        pull_request_number: 2110,
        head_repository_id: 41,
        head_ref: "feature/review".to_owned(),
        head_sha: HEAD_SHA.to_owned(),
        head_tree_sha: TREE_SHA.to_owned(),
        previous_base_ref: "main".to_owned(),
        previous_base_sha: BASE_SHA.to_owned(),
        base_history: prior_history(),
        target_base_ref: "sea/integration".to_owned(),
        policy_digest: "2".repeat(64),
        commit_identity: identity,
    })
}

pub(in crate::journal::pr_source_transition) fn retarget_receipt() -> RetargetResponseReceipt {
    RetargetResponseReceipt {
        pull_request_number: 2110,
        base_repository_id: 41,
        head_repository_id: 41,
        head_ref: "feature/review".to_owned(),
        head_sha: HEAD_SHA.to_owned(),
        previous_base_ref: "main".to_owned(),
        current_base_ref: "sea/integration".to_owned(),
        previous_base_sha: BASE_SHA.to_owned(),
        current_base_sha: TARGET_SHA.to_owned(),
    }
}

fn prior_history() -> PullRequestBaseHistory {
    PullRequestBaseHistory {
        number: 2110,
        repository_id: 41,
        head_repository_id: 41,
        head_ref: "feature/review".to_owned(),
        head_sha: HEAD_SHA.to_owned(),
        base_repository_id: 41,
        base_ref: "main".to_owned(),
        base_sha: BASE_SHA.to_owned(),
        updated_at: "2025-01-01T03:04:05Z".to_owned(),
        events: vec![BaseRefChangedEvent {
            id: "prior-event".to_owned(),
            database_id: Some(1),
            created_at: "2024-12-31T03:04:05Z".to_owned(),
            previous_ref_name: "main-old".to_owned(),
            current_ref_name: "main".to_owned(),
        }],
    }
}

pub(in crate::journal::pr_source_transition) fn retarget_history() -> PullRequestBaseHistory {
    let mut history = prior_history();
    history.base_ref = "sea/integration".to_owned();
    history.base_sha = TARGET_SHA.to_owned();
    history.updated_at = EVENT_TIME.to_owned();
    history.events.push(BaseRefChangedEvent {
        id: "retarget-event".to_owned(),
        database_id: Some(2),
        created_at: EVENT_TIME.to_owned(),
        previous_ref_name: "main".to_owned(),
        current_ref_name: "sea/integration".to_owned(),
    });
    history
}

fn nonce_commit(nonce: &str) -> GitCommit {
    let message = format!(
        "Velnor pull request validation\n\nvelnor-pr-validation-nonce:v1:{nonce}\n\nSigned-off-by: Configured Person <person@example.com>"
    );
    GitCommit {
        sha: COMMIT_SHA.to_owned(),
        message,
        author: Some(GitCommitPerson {
            name: "Configured Person".to_owned(),
            email: "person@example.com".to_owned(),
        }),
        committer: Some(GitCommitPerson {
            name: "Configured Person".to_owned(),
            email: "person@example.com".to_owned(),
        }),
        tree: GitCommitTree {
            sha: TREE_SHA.to_owned(),
        },
        parents: vec![GitCommitParent {
            sha: HEAD_SHA.to_owned(),
        }],
    }
}

fn acknowledged_ledger() -> Result<TransitionLedger, TransitionError> {
    let mut ledger = TransitionLedger::new(plan()?)?;
    ledger.claim_retarget()?;
    assert_eq!(
        ledger.record_retarget_response(retarget_receipt())?,
        ResponseDisposition::Recorded
    );
    let history = RetargetHistoryReceipt::from_history(
        &retarget_history(),
        ledger.plan(),
        &retarget_receipt(),
    )?;
    assert_eq!(
        ledger.record_retarget_verification(history)?,
        ResponseDisposition::Recorded
    );
    Ok(ledger)
}

#[test]
fn nonce_is_created_after_ack_and_full_transition_replays_exactly() -> Result<(), TransitionError> {
    let mut ledger = TransitionLedger::new(plan()?)?;
    assert_eq!(
        ledger.reserve_nonce(),
        Err(TransitionError::IncorrectHistory)
    );
    ledger.claim_retarget()?;
    assert_eq!(
        ledger.record_retarget_response(retarget_receipt())?,
        ResponseDisposition::Recorded
    );
    let history = RetargetHistoryReceipt::from_history(
        &retarget_history(),
        ledger.plan(),
        &retarget_receipt(),
    )?;
    assert_eq!(
        ledger.record_retarget_verification(history)?,
        ResponseDisposition::Recorded
    );
    let nonce = ledger.reserve_nonce()?;
    assert_eq!(nonce.len(), 64);
    assert!(
        nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    ledger.claim_nonce_commit()?;
    assert_eq!(
        ledger.record_nonce_commit(&nonce_commit(&nonce))?,
        ResponseDisposition::Recorded
    );
    ledger.claim_reference_update()?;
    assert_eq!(
        ledger.record_reference_update(ReferenceReceipt {
            reference: "refs/heads/feature/review".to_owned(),
            object_type: "commit".to_owned(),
            sha: COMMIT_SHA.to_owned(),
        })?,
        ResponseDisposition::Recorded
    );
    assert_eq!(ledger.stage(), TransitionStage::Complete);

    let plan = ledger.plan().clone();
    let events = ledger.events().to_vec();
    let replay = TransitionLedger::from_events(plan, events)?;
    assert_eq!(replay, ledger);
    Ok(())
}

#[test]
fn a_claimed_effect_cannot_be_repeated_and_uncertainty_stays_terminal()
-> Result<(), TransitionError> {
    let mut ledger = TransitionLedger::new(plan()?)?;
    ledger.claim_retarget()?;
    assert_eq!(
        ledger.claim_retarget(),
        Err(TransitionError::IncorrectHistory)
    );
    ledger.retarget_uncertain(UncertaintyReason::RestartedInFlight)?;
    assert_eq!(ledger.stage(), TransitionStage::Uncertain);
    assert_eq!(
        ledger.reserve_nonce(),
        Err(TransitionError::IncorrectHistory)
    );
    assert_eq!(
        ledger.claim_retarget(),
        Err(TransitionError::IncorrectHistory)
    );
    let replay = TransitionLedger::from_events(ledger.plan().clone(), ledger.events().to_vec())?;
    assert_eq!(replay.stage(), TransitionStage::Uncertain);
    Ok(())
}

#[test]
fn mismatched_retarget_or_commit_response_is_sticky_uncertainty() -> Result<(), TransitionError> {
    let mut retarget = TransitionLedger::new(plan()?)?;
    retarget.claim_retarget()?;
    let mut wrong = retarget_receipt();
    wrong.head_repository_id += 1;
    assert_eq!(
        retarget.record_retarget_response(wrong)?,
        ResponseDisposition::Uncertain
    );
    assert_eq!(retarget.stage(), TransitionStage::Uncertain);

    let mut commit = acknowledged_ledger()?;
    let nonce = commit.reserve_nonce()?;
    commit.claim_nonce_commit()?;
    let mut wrong_commit = nonce_commit(&nonce);
    if let Some(person) = wrong_commit.author.as_mut() {
        person.name = "Other".to_owned();
    }
    assert_eq!(
        commit.record_nonce_commit(&wrong_commit)?,
        ResponseDisposition::Uncertain
    );
    assert_eq!(commit.stage(), TransitionStage::Uncertain);

    let mut malformed = TransitionLedger::new(plan()?)?;
    malformed.claim_retarget()?;
    let mut response = retarget_receipt();
    response.current_base_sha = "not-a-sha".to_owned();
    assert_eq!(
        malformed.record_retarget_response(response)?,
        ResponseDisposition::Uncertain
    );
    assert_eq!(malformed.stage(), TransitionStage::Uncertain);
    Ok(())
}

#[test]
fn observed_retarget_after_restart_only_allows_read_only_history_check()
-> Result<(), TransitionError> {
    let mut ledger = TransitionLedger::new(plan()?)?;
    ledger.claim_retarget()?;
    assert_eq!(
        ledger.record_retarget_response(retarget_receipt())?,
        ResponseDisposition::Recorded
    );
    let mut reopened =
        TransitionLedger::from_events(ledger.plan().clone(), ledger.events().to_vec())?;
    let event_count = reopened.events().len();
    assert_eq!(
        reopened.recover_after_restart()?,
        super::ledger::RecoveryAction::VerifyRetargetHistoryReadOnly
    );
    assert_eq!(reopened.events().len(), event_count);
    assert_eq!(
        reopened.claim_retarget(),
        Err(TransitionError::IncorrectHistory)
    );
    let history = RetargetHistoryReceipt::from_history(
        &retarget_history(),
        reopened.plan(),
        &retarget_receipt(),
    )?;
    assert_eq!(
        reopened.record_retarget_verification(history)?,
        ResponseDisposition::Recorded
    );
    assert_eq!(reopened.stage(), TransitionStage::Retargeted);
    Ok(())
}

#[test]
fn wrong_nonce_parent_tree_and_reference_receipts_stay_uncertain() -> Result<(), TransitionError> {
    for mutation in [0, 1, 2] {
        let mut ledger = acknowledged_ledger()?;
        let nonce = ledger.reserve_nonce()?;
        ledger.claim_nonce_commit()?;
        let mut commit = nonce_commit(&nonce);
        match mutation {
            0 => commit.message.push_str("\ninvalid nonce"),
            1 => commit.parents[0].sha = "ffffffffffffffffffffffffffffffffffffffff".to_owned(),
            _ => commit.tree.sha = "ffffffffffffffffffffffffffffffffffffffff".to_owned(),
        }
        assert_eq!(
            ledger.record_nonce_commit(&commit)?,
            ResponseDisposition::Uncertain
        );
        assert_eq!(ledger.stage(), TransitionStage::Uncertain);
    }

    let mut ledger = acknowledged_ledger()?;
    let nonce = ledger.reserve_nonce()?;
    ledger.claim_nonce_commit()?;
    assert_eq!(
        ledger.record_nonce_commit(&nonce_commit(&nonce))?,
        ResponseDisposition::Recorded
    );
    ledger.claim_reference_update()?;
    assert_eq!(
        ledger.record_reference_update(ReferenceReceipt {
            reference: "refs/heads/other".to_owned(),
            object_type: "commit".to_owned(),
            sha: COMMIT_SHA.to_owned(),
        })?,
        ResponseDisposition::Uncertain
    );
    assert_eq!(ledger.stage(), TransitionStage::Uncertain);
    Ok(())
}

#[test]
fn event_replay_rejects_unknown_fields_and_out_of_order_events() -> Result<(), TransitionError> {
    let mut serialized = serde_json::to_value(vec![super::types::TransitionEvent::NonceReserved {
        nonce: "a".repeat(64),
    }])
    .map_err(|_| TransitionError::IncorrectHistory)?;
    serialized[0]["unexpected"] = Value::Bool(true);
    assert!(serde_json::from_value::<Vec<super::types::TransitionEvent>>(serialized).is_err());
    assert_eq!(
        TransitionLedger::from_events(
            plan()?,
            vec![super::types::TransitionEvent::NonceReserved {
                nonce: "a".repeat(64),
            }],
        ),
        Err(TransitionError::IncorrectHistory)
    );
    Ok(())
}

#[test]
fn response_requires_exact_complete_timeline_and_ack_replays_exact_response()
-> Result<(), TransitionError> {
    let mut ledger = TransitionLedger::new(plan()?)?;
    ledger.claim_retarget()?;
    let response = retarget_receipt();
    assert_eq!(
        ledger.record_retarget_response(response.clone())?,
        ResponseDisposition::Recorded
    );
    let mut wrong_history = retarget_history();
    wrong_history.events.push(BaseRefChangedEvent {
        id: "extra-event".to_owned(),
        database_id: Some(3),
        created_at: "2025-01-03T03:04:05Z".to_owned(),
        previous_ref_name: "sea/integration".to_owned(),
        current_ref_name: "sea/later".to_owned(),
    });
    wrong_history.base_ref = "sea/later".to_owned();
    assert_eq!(
        RetargetHistoryReceipt::from_history(&wrong_history, ledger.plan(), &response),
        Err(TransitionError::IncorrectHistory)
    );
    let history =
        RetargetHistoryReceipt::from_history(&retarget_history(), ledger.plan(), &response)?;
    assert_eq!(
        ledger.record_retarget_verification(history)?,
        ResponseDisposition::Recorded
    );
    let mut forged = ledger.events().to_vec();
    if let Some(super::types::TransitionEvent::RetargetAcknowledged { response, .. }) =
        forged.last_mut()
    {
        response.current_base_sha = BASE_SHA.to_owned();
    }
    assert_eq!(
        TransitionLedger::from_events(ledger.plan().clone(), forged),
        Err(TransitionError::IncorrectHistory)
    );
    Ok(())
}
