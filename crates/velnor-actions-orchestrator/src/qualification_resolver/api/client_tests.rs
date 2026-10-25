use super::{allows_unprotected_default, branch_protection_matches};
use velnor_actions_contract::{
    QualificationDispatch, QualificationDispatchMode, QualificationPhase,
    verification_repeat_campaign,
};

const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";

fn verification_dispatch() -> QualificationDispatch {
    let workflow_ref = format!(
        "owner/project/{}@refs/heads/main",
        velnor_actions_contract::workflow::QUALIFICATION_WORKFLOW_PATH
    );
    let campaign = verification_repeat_campaign(
        "owner/project",
        "main",
        "refs/heads/main",
        &workflow_ref,
        SOURCE,
        SOURCE,
        41,
    );
    QualificationDispatch {
        mode: QualificationDispatchMode::VerificationOnly,
        campaign,
        phase: QualificationPhase::Cold,
        repository: "owner/project".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: false,
        workflow_ref,
        workflow_sha: SOURCE.to_owned(),
        source_sha: SOURCE.to_owned(),
        run_id: 41,
        run_attempt: 1,
        predecessor: None,
        scale_set_image_source_sha: None,
    }
}

#[test]
fn unprotected_default_requires_the_bound_verification_wrapper() {
    let context = verification_dispatch();
    let allowed = allows_unprotected_default(&context, "owner/project", "main", None, SOURCE)
        .expect("exact verification dispatch");

    assert!(allowed);
    assert!(branch_protection_matches(Some(false), false, true, allowed));
    assert!(!branch_protection_matches(Some(true), false, true, allowed));
    assert!(!branch_protection_matches(None, false, true, allowed));
}

#[test]
fn unprotected_controlled_default_remains_rejected() {
    let mut context = verification_dispatch();
    context.mode = QualificationDispatchMode::Controlled;

    assert!(allows_unprotected_default(&context, "owner/project", "main", None, SOURCE,).is_err());
    assert!(!branch_protection_matches(Some(false), false, true, false));
}

#[test]
fn verification_exception_rejects_wrong_path_source_or_branch() {
    let context = verification_dispatch();
    assert!(
        allows_unprotected_default(
            &context,
            "owner/project",
            "main",
            None,
            "1123456789abcdef0123456789abcdef01234567",
        )
        .is_err()
    );

    let mut wrong_path = context.clone();
    wrong_path.workflow_ref = format!(
        "owner/project/{}@refs/heads/main",
        velnor_actions_workflow_renderer::render::WORKFLOW_PATH
    );
    assert!(
        allows_unprotected_default(&wrong_path, "owner/project", "main", None, SOURCE,).is_err()
    );

    let mut wrong_branch = context;
    wrong_branch.git_ref = "refs/heads/feature".to_owned();
    wrong_branch.workflow_ref = format!(
        "owner/project/{}@{}",
        velnor_actions_contract::workflow::QUALIFICATION_WORKFLOW_PATH,
        wrong_branch.git_ref
    );
    wrong_branch.campaign = verification_repeat_campaign(
        &wrong_branch.repository,
        &wrong_branch.default_branch,
        &wrong_branch.git_ref,
        &wrong_branch.workflow_ref,
        &wrong_branch.workflow_sha,
        &wrong_branch.source_sha,
        wrong_branch.run_id,
    );
    assert!(
        allows_unprotected_default(
            &wrong_branch,
            "owner/project",
            "main",
            Some("sea/integration"),
            SOURCE,
        )
        .is_err()
    );
}

#[test]
fn configured_unprotected_verification_branch_keeps_existing_admission() {
    let mut context = verification_dispatch();
    context.git_ref = "refs/heads/sea/integration".to_owned();
    context.workflow_ref = format!(
        "owner/project/{}@{}",
        velnor_actions_contract::workflow::QUALIFICATION_WORKFLOW_PATH,
        context.git_ref
    );
    context.campaign = verification_repeat_campaign(
        &context.repository,
        &context.default_branch,
        &context.git_ref,
        &context.workflow_ref,
        &context.workflow_sha,
        &context.source_sha,
        context.run_id,
    );
    let allowed_default = allows_unprotected_default(
        &context,
        "owner/project",
        "main",
        Some("sea/integration"),
        SOURCE,
    )
    .expect("configured verification branch");

    assert!(!allowed_default);
    assert!(branch_protection_matches(
        Some(false),
        false,
        false,
        allowed_default
    ));
}
