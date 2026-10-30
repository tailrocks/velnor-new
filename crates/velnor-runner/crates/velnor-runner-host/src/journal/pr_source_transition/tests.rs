use serde_json::Value;
use velnor_runner_github::workflow_run::{
    GitCommit, GitCommitIdentity, GitCommitParent, GitCommitPerson, GitCommitTree,
};

use super::ledger::TransitionLedger;
use super::plan::{TransitionPlan, TransitionPlanInput};
use super::receipt::{ReferenceReceipt, RetargetReceipt};
use super::types::{ResponseDisposition, TransitionError, TransitionStage, UncertaintyReason};

const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TARGET_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const COMMIT_SHA: &str = "dddddddddddddddddddddddddddddddddddddddd";
const TREE_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn plan() -> Result<TransitionPlan, TransitionError> {
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
        previous_base_ref: "main".to_owned(),
        previous_base_sha: BASE_SHA.to_owned(),
        target_base_ref: "sea/integration".to_owned(),
        policy_digest: "2".repeat(64),
        commit_identity: identity,
    })
}

fn retarget_receipt() -> RetargetReceipt {
    RetargetReceipt {
        pull_request_number: 2110,
        base_repository_id: 41,
        head_repository_id: 41,
        head_ref: "feature/review".to_owned(),
        head_sha: HEAD_SHA.to_owned(),
        previous_base_ref: "main".to_owned(),
        current_base_ref: "sea/integration".to_owned(),
        base_event_id: "PRRC_opaque+identity=".to_owned(),
        base_sha: TARGET_SHA.to_owned(),
    }
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
        ledger.record_retarget(retarget_receipt())?,
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
        ledger.record_retarget(retarget_receipt())?,
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
        retarget.record_retarget(wrong)?,
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
    Ok(())
}

#[test]
fn invalid_or_missing_configured_identity_fails_deserialization() -> Result<(), TransitionError> {
    let plan = plan()?;
    let serialized = serde_json::to_value(&plan).map_err(|_| TransitionError::IncorrectPlan)?;
    let Value::Object(mut fields) = serialized else {
        return Err(TransitionError::IncorrectPlan);
    };
    fields.remove("commit_identity");
    assert!(serde_json::from_value::<TransitionPlan>(Value::Object(fields)).is_err());

    let mut value = serde_json::to_value(plan).map_err(|_| TransitionError::IncorrectPlan)?;
    value["commit_identity"]["name"] = Value::String("".to_owned());
    assert!(serde_json::from_value::<TransitionPlan>(value).is_err());
    Ok(())
}

#[test]
fn plan_digest_binds_policy_identity_and_target() -> Result<(), TransitionError> {
    let first = plan()?;
    let second = plan()?;
    assert_eq!(first.digest()?, second.digest()?);
    let mut altered = first.clone();
    altered.policy_digest = "3".repeat(64);
    assert_ne!(first.digest()?, altered.digest()?);
    Ok(())
}

#[test]
fn transition_plan_rejects_fork_head() -> Result<(), TransitionError> {
    let mut plan = plan()?;
    plan.head_repository_id += 1;
    assert_eq!(plan.validate(), Err(TransitionError::IncorrectPlan));
    Ok(())
}

#[test]
fn transition_plan_rejects_noop_base_change() -> Result<(), TransitionError> {
    let mut plan = plan()?;
    plan.target_base_ref = plan.previous_base_ref.clone();
    assert_eq!(plan.validate(), Err(TransitionError::IncorrectPlan));
    Ok(())
}
