use super::common::{
    REST_WORKFLOW_PATH, TRUSTED_WORKFLOW_REF, batch, offer, policy_view, valid_rule, valid_run,
};
use crate::policy::{
    JobTrustEvidence, JobTrustRuleView, PolicyGap, PolicyMismatch, ReusableWorkflowEvidence,
    ReusableWorkflowRuleView, WorkflowTrustField, verify_job_offer,
};

#[test]
fn verifies_exact_event_tuple_without_binding_request_to_runner() {
    static NO_REUSABLES: [ReusableWorkflowRuleView<'static>; 0] = [];
    let rule = valid_rule(&NO_REUSABLES);
    let policy = policy_view(std::slice::from_ref(&rule));
    let events = batch(7, &[offer(42, Some(TRUSTED_WORKFLOW_REF))]);

    let JobTrustEvidence::Verified(verified) = verify_job_offer(&events, 0, &valid_run(), &policy)
    else {
        panic!("expected verified trust tuple");
    };

    assert_eq!(verified.message_id(), 7);
    assert_eq!(verified.runner_request_id(), 42);
    assert_eq!(verified.scale_set_job_id(), Some("opaque:42"));
    assert_eq!(verified.workflow_run_id(), 88);
    assert_eq!(verified.job_workflow_ref(), rule.job_workflow_ref);
    assert_eq!(verified.workflow_ref(), rule.workflow_ref);
    assert_eq!(
        verified.head_repository_full_name(),
        "ChainArgos/java-monorepo"
    );
    assert_eq!(verified.head_branch(), "main");
    assert_eq!(verified.policy_digest(), "sha256:policy-1");
}

#[test]
fn rest_path_keeps_full_ref_while_coarse_allowlist_remains_bare_path() {
    static NO_REUSABLES: [ReusableWorkflowRuleView<'static>; 0] = [];
    let rule = valid_rule(&NO_REUSABLES);
    let policy = policy_view(std::slice::from_ref(&rule));
    let events = batch(7, &[offer(42, Some(TRUSTED_WORKFLOW_REF))]);

    assert!(matches!(
        verify_job_offer(&events, 0, &valid_run(), &policy),
        JobTrustEvidence::Verified(_)
    ));

    let mut changed_run = valid_run();
    changed_run.path = format!("{REST_WORKFLOW_PATH}-other");
    assert_eq!(
        verify_job_offer(&events, 0, &changed_run, &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::WorkflowReferenceMismatch)
    );

    for path_without_exact_ref in [
        ".github/workflows/ci.yml",
        ".github/workflows/ci.yml@",
        ".github/workflows/ci.yml@refs/heads/other",
    ] {
        let mut changed_run = valid_run();
        changed_run.path = path_without_exact_ref.to_owned();
        assert_eq!(
            verify_job_offer(&events, 0, &changed_run, &policy),
            JobTrustEvidence::Rejected(PolicyMismatch::WorkflowReferenceMismatch)
        );
    }
}

#[test]
fn current_rest_attempt_is_not_attributed_to_scale_set_event() {
    static NO_REUSABLES: [ReusableWorkflowRuleView<'static>; 0] = [];
    let rule = valid_rule(&NO_REUSABLES);
    let policy = policy_view(std::slice::from_ref(&rule));
    let events = batch(7, &[offer(42, Some(TRUSTED_WORKFLOW_REF))]);
    let baseline = verify_job_offer(&events, 0, &valid_run(), &policy);

    let mut changed_observation = valid_run();
    changed_observation.observed_run_attempt = 99;
    assert_eq!(
        verify_job_offer(&events, 0, &changed_observation, &policy),
        baseline
    );
}

#[test]
fn missing_or_invalid_source_and_workflow_fields_remain_unknown() {
    static NO_REUSABLES: [ReusableWorkflowRuleView<'static>; 0] = [];
    let rule = valid_rule(&NO_REUSABLES);
    let policy = policy_view(std::slice::from_ref(&rule));

    let events = batch(7, &[offer(42, Some(TRUSTED_WORKFLOW_REF))]);
    let mut run = valid_run();
    run.head_repository_full_name = WorkflowTrustField::Missing;
    assert_eq!(
        verify_job_offer(&events, 0, &run, &policy),
        JobTrustEvidence::Unknown(PolicyGap::MissingField)
    );

    let mut run = valid_run();
    run.referenced_workflows = WorkflowTrustField::Missing;
    assert_eq!(
        verify_job_offer(&events, 0, &run, &policy),
        JobTrustEvidence::Unknown(PolicyGap::MissingField)
    );

    let missing_ref = batch(7, &[offer(42, None)]);
    assert_eq!(
        verify_job_offer(&missing_ref, 0, &valid_run(), &policy),
        JobTrustEvidence::Unknown(PolicyGap::MissingField)
    );

    let mut invalid_ref = offer(42, None);
    invalid_ref["jobWorkflowRef"] = serde_json::json!([]);
    let invalid_batch = batch(7, &[invalid_ref]);
    assert_eq!(
        verify_job_offer(&invalid_batch, 0, &valid_run(), &policy),
        JobTrustEvidence::Unknown(PolicyGap::InvalidField)
    );
}

#[test]
fn event_indices_cannot_pair_jobs_with_other_events_trust_metadata() {
    static NO_REUSABLES: [ReusableWorkflowRuleView<'static>; 0] = [];
    let rule = valid_rule(&NO_REUSABLES);
    let policy = policy_view(std::slice::from_ref(&rule));

    let trusted = batch(
        11,
        &[
            offer(41, Some(TRUSTED_WORKFLOW_REF)),
            offer(
                42,
                Some("attacker/repo/.github/workflows/ci.yml@refs/heads/main"),
            ),
        ],
    );
    let foreign = batch(
        12,
        &[offer(
            99,
            Some("attacker/repo/.github/workflows/ci.yml@refs/heads/main"),
        )],
    );

    let JobTrustEvidence::Verified(first) = verify_job_offer(&trusted, 0, &valid_run(), &policy)
    else {
        panic!("first trusted event must verify");
    };
    assert_eq!(first.runner_request_id(), 41);
    let paired_untrusted = trusted.event(1).expect("second trusted-batch event");
    assert_eq!(paired_untrusted.job().request_id, Some(42));
    assert_eq!(
        paired_untrusted.job_workflow_ref(),
        &WorkflowTrustField::Present(
            "attacker/repo/.github/workflows/ci.yml@refs/heads/main".to_owned()
        )
    );
    assert_eq!(
        verify_job_offer(&trusted, 1, &valid_run(), &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::WorkflowReferenceMismatch)
    );

    let paired_foreign = foreign.event(0).expect("foreign-batch event");
    assert_eq!(paired_foreign.job().request_id, Some(99));
    assert_eq!(
        paired_foreign.job_workflow_ref(),
        &WorkflowTrustField::Present(
            "attacker/repo/.github/workflows/ci.yml@refs/heads/main".to_owned()
        )
    );
    assert_eq!(
        verify_job_offer(&foreign, 0, &valid_run(), &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::WorkflowReferenceMismatch)
    );
    assert_eq!(
        verify_job_offer(&trusted, 99, &valid_run(), &policy),
        JobTrustEvidence::Unknown(PolicyGap::InvalidEventIndex)
    );
}

#[test]
fn mismatched_event_repository_and_fork_sources_are_rejected() {
    static NO_REUSABLES: [ReusableWorkflowRuleView<'static>; 0] = [];
    let rule = valid_rule(&NO_REUSABLES);
    let policy = policy_view(std::slice::from_ref(&rule));
    let events = batch(7, &[offer(42, Some(TRUSTED_WORKFLOW_REF))]);

    let mut run = valid_run();
    run.event = "pull_request".to_owned();
    assert_eq!(
        verify_job_offer(&events, 0, &run, &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::EventRepositoryMismatch)
    );

    let mut run = valid_run();
    run.head_repository_full_name = WorkflowTrustField::Present("fork/clone".to_owned());
    assert_eq!(
        verify_job_offer(&events, 0, &run, &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::ForkSourceMismatch)
    );

    let wrong_repository = batch(
        7,
        &[serde_json::json!({
            "messageType": "JobAvailable",
            "runnerRequestId": 42,
            "workflowRunId": 88,
            "ownerName": "ChainArgos",
            "repositoryName": "other",
            "eventName": "push",
            "jobWorkflowRef": TRUSTED_WORKFLOW_REF
        })],
    );
    assert_eq!(
        verify_job_offer(&wrong_repository, 0, &valid_run(), &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::EventRepositoryMismatch)
    );
}

#[test]
fn reusable_workflow_chain_is_exact_and_ordered() {
    let expected = [ReusableWorkflowRuleView {
        path: "ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1",
        git_ref: "refs/tags/v1",
        sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    }];
    let root_rule = valid_rule(&expected);
    let rule = JobTrustRuleView {
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/reuse.yml@refs/tags/v1",
        ..root_rule
    };
    let policy = policy_view(std::slice::from_ref(&rule));
    let events = batch(7, &[offer(42, Some(rule.job_workflow_ref))]);
    let mut run = valid_run();
    run.referenced_workflows = WorkflowTrustField::Present(vec![ReusableWorkflowEvidence {
        path: expected[0].path.to_owned(),
        git_ref: WorkflowTrustField::Present(expected[0].git_ref.to_owned()),
        sha: expected[0].sha.to_owned(),
    }]);
    assert!(matches!(
        verify_job_offer(&events, 0, &run, &policy),
        JobTrustEvidence::Verified(_)
    ));

    if let WorkflowTrustField::Present(workflows) = &mut run.referenced_workflows {
        workflows[0].sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    }
    assert_eq!(
        verify_job_offer(&events, 0, &run, &policy),
        JobTrustEvidence::Rejected(PolicyMismatch::WorkflowReferenceMismatch)
    );
}
