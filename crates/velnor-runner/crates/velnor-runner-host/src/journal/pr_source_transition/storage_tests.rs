use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::request_lifecycle::tests::helpers::Scratch;
use std::collections::VecDeque;
use velnor_runner_github::workflow_run::{
    GitCommit, GitCommitParent, GitCommitPerson, GitCommitTree, PullRequestBaseUpdateRequest,
    PullRequestNonceCommitReceipt, RepositoryTarget, update_pull_request_base,
};
use velnor_runner_github::{Exchange, SessionRequest, Transport, TransportFail};

use super::{TransitionEffect, TransitionRecovery, TransitionStage};

struct Script(VecDeque<Result<Exchange, TransportFail>>);

impl Transport for Script {
    fn exchange(&mut self, _: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.0.pop_front().unwrap_or(Err(TransportFail::Reset))
    }
}

fn repository_body(repository_id: i64) -> Result<Vec<u8>, HostError> {
    serde_json::to_vec(&serde_json::json!({
        "id": repository_id,
        "name": "java-monorepo",
        "full_name": "ChainArgos/java-monorepo",
        "private": true,
        "owner": {"login": "ChainArgos"}
    }))
    .map_err(|_| HostError::Journal)
}

fn pull_request_body(
    base_ref: &str,
    base_sha: &str,
    base_repository_id: i64,
) -> Result<Vec<u8>, HostError> {
    serde_json::to_vec(&serde_json::json!({
        "number": 2110,
        "state": "open",
        "merged": false,
        "head": {"ref": "feature/review", "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "repo": {"id": 41, "name": "java-monorepo", "full_name": "ChainArgos/java-monorepo", "private": true, "owner": {"login": "ChainArgos"}}},
        "base": {"ref": base_ref, "sha": base_sha, "repo": {"id": base_repository_id, "name": "java-monorepo", "full_name": "ChainArgos/java-monorepo", "private": true, "owner": {"login": "ChainArgos"}}},
        "merge_commit_sha": null
    }))
    .map_err(|_| HostError::Journal)
}

fn base_update_receipt_for_repository(
    plan: &super::TransitionPlan,
    target_repository_id: i64,
) -> Result<velnor_runner_github::workflow_run::PullRequestBaseUpdateReceipt, HostError> {
    let response = |body| Ok(Exchange { status: 200, body });
    let mut script = Script(VecDeque::from([
        response(repository_body(target_repository_id)?),
        response(pull_request_body(
            "main",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            target_repository_id,
        )?),
        response(pull_request_body(
            "sea/integration",
            "cccccccccccccccccccccccccccccccccccccccc",
            target_repository_id,
        )?),
        response(repository_body(target_repository_id)?),
    ]));
    update_pull_request_base(
        &mut script,
        RepositoryTarget {
            id: target_repository_id,
            owner: &plan.repository_owner,
            name: &plan.repository_name,
        },
        PullRequestBaseUpdateRequest {
            number: plan.pull_request_number,
            head_repository_id: plan.head_repository_id,
            head_ref: plan.head_ref.clone(),
            head_sha: plan.head_sha.clone(),
            previous_base_ref: plan.previous_base_ref.clone(),
            previous_base_sha: plan.previous_base_sha.clone(),
            target_base_ref: plan.target_base_ref.clone(),
        },
        "token",
    )
    .map_err(|_| HostError::Journal)
}

fn base_update_receipt(
    plan: &super::TransitionPlan,
) -> Result<velnor_runner_github::workflow_run::PullRequestBaseUpdateReceipt, HostError> {
    base_update_receipt_for_repository(plan, plan.repository_id)
}

#[tokio::test]
async fn committed_effect_claim_survives_restart_and_recovery_blocks_retry() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let api = journal.pr_source_transition();
    let plan = super::model::tests::plan().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    let plan_digest = plan.digest().map_err(|_| HostError::Journal)?;
    assert_eq!(
        api.begin(plan.clone()).await?.stage(),
        TransitionStage::Prepared
    );
    let mut changed_plan = plan.clone();
    changed_plan.policy_digest = "3".repeat(64);
    assert!(api.begin(changed_plan).await.is_err());
    let mut changed_binding = plan.clone();
    changed_binding.binding_digest = "4".repeat(64);
    assert!(api.begin(changed_binding).await.is_err());
    let mut changed_scale_set = plan.clone();
    changed_scale_set.scale_set_id += 1;
    assert!(api.begin(changed_scale_set).await.is_err());

    let claim = api.claim_retarget(&transition_id, &plan_digest, 0).await?;
    assert_eq!(claim.effect(), TransitionEffect::Retarget);
    assert_eq!(claim.event_count(), 1);
    assert!(
        api.claim_retarget(&transition_id, &plan_digest, 0)
            .await
            .is_err()
    );
    assert_eq!(
        api.load(&transition_id).await?.stage(),
        TransitionStage::RetargetInFlight
    );
    drop(journal);

    let journal = Journal::open(&path).await?;
    let api = journal.pr_source_transition();
    assert_eq!(
        api.load(&transition_id).await?.stage(),
        TransitionStage::RetargetInFlight
    );
    assert_eq!(
        api.recover_after_restart(&transition_id, &plan_digest, 1)
            .await?,
        TransitionRecovery::RecordedUncertainty
    );
    let recovered = api.load(&transition_id).await?;
    assert_eq!(recovered.stage(), TransitionStage::Uncertain);
    assert_eq!(recovered.event_count(), 2);
    assert!(
        api.claim_retarget(&transition_id, &plan_digest, 2)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn failed_event_insert_rolls_back_and_returns_no_effect_claim() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    let api = journal.pr_source_transition();
    let plan = super::model::tests::plan().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    let plan_digest = plan.digest().map_err(|_| HostError::Journal)?;
    api.begin(plan).await?;

    journal
        .connection()
        .await?
        .execute(
            "CREATE TRIGGER reject_transition_claim BEFORE INSERT ON pr_source_transition_events BEGIN SELECT RAISE(ABORT, 'test persistence failure'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(
        api.claim_retarget(&transition_id, &plan_digest, 0)
            .await
            .is_err()
    );
    assert_eq!(
        api.load(&transition_id).await?.stage(),
        TransitionStage::Prepared
    );
    let mut rows = journal
        .connection()
        .await?
        .query(
            "SELECT COUNT(*) FROM pr_source_transition_events WHERE transition_id = ?1",
            [transition_id.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.get::<i64>(0).map_err(|_| HostError::Journal)?, 0);
    Ok(())
}

fn nonce_receipt(plan: &super::TransitionPlan, nonce: String) -> PullRequestNonceCommitReceipt {
    let message = format!(
        "Velnor pull request validation\n\nvelnor-pr-validation-nonce:v1:{nonce}\n\n{}",
        plan.commit_identity.sign_off_line()
    );
    let identity = plan.commit_identity.clone();
    PullRequestNonceCommitReceipt {
        head_repository_id: plan.head_repository_id,
        head_ref: plan.head_ref.clone(),
        parent_sha: plan.head_sha.clone(),
        nonce,
        identity: identity.clone(),
        commit: GitCommit {
            sha: "dddddddddddddddddddddddddddddddddddddddd".to_owned(),
            message,
            author: Some(GitCommitPerson {
                name: identity.name().to_owned(),
                email: identity.email().to_owned(),
            }),
            committer: Some(GitCommitPerson {
                name: identity.name().to_owned(),
                email: identity.email().to_owned(),
            }),
            tree: GitCommitTree {
                sha: plan.head_tree_sha.clone(),
            },
            parents: vec![GitCommitParent {
                sha: plan.head_sha.clone(),
            }],
        },
    }
}

#[tokio::test]
async fn complete_transition_replays_after_reopen_and_limits_receipt_capability()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let api = journal.pr_source_transition();
    let plan = super::model::tests::plan().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    let plan_digest = plan.digest().map_err(|_| HostError::Journal)?;
    api.begin(plan.clone()).await?;

    let claim = api.claim_retarget(&transition_id, &plan_digest, 0).await?;
    assert_eq!(claim.effect(), TransitionEffect::Retarget);
    let response = base_update_receipt(&plan)?;
    let snapshot = api
        .record_retarget_response(&transition_id, &plan_digest, 1, &response)
        .await?;
    assert_eq!(snapshot.stage(), TransitionStage::RetargetResponseObserved);
    let history = super::model::tests::retarget_history();
    let snapshot = api
        .record_retarget_verification(&transition_id, &plan_digest, 2, &history)
        .await?;
    assert_eq!(snapshot.stage(), TransitionStage::Retargeted);

    let nonce = api.reserve_nonce(&transition_id, &plan_digest, 3).await?;
    let claim = api
        .claim_nonce_commit(&transition_id, &plan_digest, 4)
        .await?;
    assert_eq!(claim.effect(), TransitionEffect::NonceCommit);
    assert_eq!(claim.nonce(), Some(nonce.as_str()));
    let receipt = nonce_receipt(&plan, nonce);
    let snapshot = api
        .record_nonce_commit(&transition_id, &plan_digest, 5, &receipt)
        .await?;
    assert_eq!(snapshot.stage(), TransitionStage::NonceCommitCreated);
    assert!(snapshot.nonce_commit_receipt().is_some());

    let claim = api
        .claim_reference_update(&transition_id, &plan_digest, 6)
        .await?;
    assert_eq!(claim.effect(), TransitionEffect::PublishReference);
    let reference = velnor_runner_github::workflow_run::GitReference {
        reference: format!("refs/heads/{}", plan.head_ref),
        object: velnor_runner_github::workflow_run::GitReferenceObject {
            object_type: "commit".to_owned(),
            sha: receipt.commit.sha,
        },
    };
    let snapshot = api
        .record_reference_update(&transition_id, &plan_digest, 7, &reference)
        .await?;
    assert_eq!(snapshot.stage(), TransitionStage::Complete);
    assert_eq!(snapshot.event_count(), 8);
    assert!(snapshot.nonce_commit_receipt().is_none());
    drop(journal);

    let journal = Journal::open(&path).await?;
    let snapshot = journal.pr_source_transition().load(&transition_id).await?;
    assert_eq!(snapshot.stage(), TransitionStage::Complete);
    assert_eq!(snapshot.event_count(), 8);
    assert!(snapshot.nonce_commit_receipt().is_none());
    Ok(())
}

#[tokio::test]
async fn validated_foreign_base_repository_receipt_is_durably_uncertain() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let api = journal.pr_source_transition();
    let plan = super::model::tests::plan().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    let plan_digest = plan.digest().map_err(|_| HostError::Journal)?;
    api.begin(plan.clone()).await?;
    let _claim = api.claim_retarget(&transition_id, &plan_digest, 0).await?;

    let foreign_receipt = base_update_receipt_for_repository(&plan, plan.repository_id + 1)?;
    assert_eq!(foreign_receipt.base_repository_id(), plan.repository_id + 1);
    let snapshot = api
        .record_retarget_response(&transition_id, &plan_digest, 1, &foreign_receipt)
        .await?;
    assert_eq!(snapshot.stage(), TransitionStage::Uncertain);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let snapshot = journal.pr_source_transition().load(&transition_id).await?;
    assert_eq!(snapshot.stage(), TransitionStage::Uncertain);
    assert_eq!(snapshot.event_count(), 2);
    Ok(())
}

#[tokio::test]
async fn persisted_poisoned_event_is_rejected_on_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let api = journal.pr_source_transition();
    let plan = super::model::tests::plan().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    let plan_digest = plan.digest().map_err(|_| HostError::Journal)?;
    api.begin(plan).await?;
    let _claim = api.claim_retarget(&transition_id, &plan_digest, 0).await?;
    let connection = journal.connection().await?;
    connection
        .execute("DROP TRIGGER pr_source_transition_events_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE pr_source_transition_events SET event_json = ?1 WHERE transition_id = ?2 AND sequence = 0",
            (r#"{"event":"nonce_reserved","nonce":"0000000000000000000000000000000000000000000000000000000000000000"}"#, transition_id.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn persisted_poisoned_plan_is_rejected_on_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let plan = super::model::tests::plan().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    journal.pr_source_transition().begin(plan).await?;
    journal
        .connection()
        .await?
        .execute("DROP TRIGGER pr_source_transitions_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    journal
        .connection()
        .await?
        .execute(
            "UPDATE pr_source_transitions SET plan_json = ?1 WHERE transition_id = ?2",
            ("{}", transition_id.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}
