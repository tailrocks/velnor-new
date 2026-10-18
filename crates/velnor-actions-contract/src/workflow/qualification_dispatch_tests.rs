use super::{
    QualificationDispatch, QualificationDispatchMode, QualificationPhase, QualificationRunRef,
    verification_repeat_campaign,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn dispatch() -> QualificationDispatch {
    QualificationDispatch {
        mode: QualificationDispatchMode::Controlled,
        campaign: "pr18-validation".to_owned(),
        phase: QualificationPhase::Cold,
        repository: "owner/project".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: SHA.to_owned(),
        source_sha: SHA.to_owned(),
        run_id: 123,
        run_attempt: 1,
        predecessor: None,
        scale_set_image_source_sha: None,
    }
}

fn verification_repeat(attempt: u32) -> QualificationDispatch {
    let mut value = dispatch();
    value.mode = QualificationDispatchMode::VerificationOnly;
    value.workflow_ref =
        "owner/project/.github/workflows/qualification.yml@refs/heads/main".to_owned();
    value.ref_protected = false;
    value.run_attempt = attempt;
    value.phase = if attempt == 1 {
        QualificationPhase::Cold
    } else {
        QualificationPhase::Warm
    };
    value.predecessor = (attempt > 1).then_some(QualificationRunRef {
        run_id: value.run_id,
        run_attempt: 1,
    });
    value.campaign = repeat_campaign(&value);
    value
}

fn repeat_campaign(value: &QualificationDispatch) -> String {
    verification_repeat_campaign(
        &value.repository,
        &value.default_branch,
        &value.git_ref,
        &value.workflow_ref,
        &value.workflow_sha,
        &value.source_sha,
        value.run_id,
    )
}

#[test]
fn image_source_is_distinct_from_the_consumer_dispatch_source() {
    let mut value = dispatch();
    value.scale_set_image_source_sha = Some("b".repeat(40));
    assert_ne!(value.scale_set_image_source_sha.as_deref(), Some(SHA));
    assert!(value.validate_for("main", "owner/project", SHA).is_ok());
}

#[test]
fn malformed_image_source_is_rejected() {
    let mut value = dispatch();
    value.scale_set_image_source_sha = Some("A".repeat(40));
    assert!(value.validate_shape().is_err());
}

#[test]
fn accepts_exact_protected_default_dispatch() {
    assert!(
        dispatch()
            .validate_for("main", "owner/project", SHA)
            .is_ok()
    );
}

#[test]
fn rejects_each_unbound_dispatch_identity() {
    let mut value = dispatch();
    value.repository = "attacker/project".to_owned();
    assert!(value.validate_for("main", "owner/project", SHA).is_err());

    let mut value = dispatch();
    value.git_ref = "refs/heads/feature".to_owned();
    assert!(value.validate_for("main", "owner/project", SHA).is_err());

    let mut value = dispatch();
    value.ref_protected = false;
    assert!(value.validate_for("main", "owner/project", SHA).is_err());

    let mut value = dispatch();
    value.workflow_ref = "owner/project/.github/workflows/other.yml@refs/heads/main".to_owned();
    assert!(value.validate_for("main", "owner/project", SHA).is_err());

    let mut value = dispatch();
    value.workflow_sha = "1123456789abcdef0123456789abcdef01234567".to_owned();
    assert!(value.validate_for("main", "owner/project", SHA).is_err());
}

#[test]
fn admits_only_the_configured_verification_wrapper_ref() {
    let mut value = dispatch();
    value.git_ref = "refs/heads/sea/integration".to_owned();
    value.ref_protected = false;
    value.workflow_ref =
        "owner/project/.github/workflows/qualification.yml@refs/heads/sea/integration".to_owned();
    assert!(
        value
            .validate_for_config("main", Some("sea/integration"), "owner/project", SHA)
            .is_ok()
    );
    assert!(
        value
            .validate_for_config("main", None, "owner/project", SHA)
            .is_err(),
        "unconfigured non-default refs must fail"
    );

    value.workflow_ref =
        "owner/project/.github/workflows/ci.yml@refs/heads/sea/integration".to_owned();
    assert!(
        value
            .validate_for_config("main", Some("sea/integration"), "owner/project", SHA)
            .is_err(),
        "the direct CI workflow cannot dispatch from the verification branch"
    );
}

#[test]
fn wrapper_dispatch_remains_protected_on_the_default_branch() {
    let mut value = dispatch();
    value.workflow_ref =
        "owner/project/.github/workflows/qualification.yml@refs/heads/main".to_owned();
    assert!(
        value
            .validate_for_config("main", Some("sea/integration"), "owner/project", SHA)
            .is_ok()
    );
    value.ref_protected = false;
    assert!(
        value
            .validate_for_config("main", Some("sea/integration"), "owner/project", SHA)
            .is_err()
    );
}

#[test]
fn verification_repeat_uses_attempt_one_cold_and_later_warm_identity() {
    let cold = verification_repeat(1);
    assert!(cold.validate_shape().is_ok());
    assert!(
        cold.validate_for_config("main", None, "owner/project", SHA)
            .is_ok(),
        "the exact unprotected default wrapper is admitted for verification-only"
    );

    let warm = verification_repeat(2);
    assert_eq!(warm.campaign, cold.campaign);
    assert_eq!(warm.phase, QualificationPhase::Warm);
    assert_eq!(
        warm.predecessor,
        Some(QualificationRunRef {
            run_id: warm.run_id,
            run_attempt: 1,
        })
    );
    assert!(warm.validate_shape().is_ok());
    assert!(verification_repeat(3).validate_shape().is_ok());
}

#[test]
fn verification_repeat_rejects_caller_chosen_phase_campaign_and_predecessor() {
    let mut value = verification_repeat(2);
    value.phase = QualificationPhase::Cold;
    assert!(value.validate_shape().is_err());

    let mut value = verification_repeat(2);
    value.campaign = "caller-selected".to_owned();
    assert!(value.validate_shape().is_err());

    let mut value = verification_repeat(2);
    value.predecessor = Some(QualificationRunRef {
        run_id: value.run_id - 1,
        run_attempt: 1,
    });
    assert!(value.validate_shape().is_err());
}

#[test]
fn verification_repeat_requires_configured_wrapper_source_and_ref() {
    let mut value = verification_repeat(1);
    value.workflow_ref = "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned();
    value.campaign = repeat_campaign(&value);
    assert!(value.validate_shape().is_err());

    let mut value = verification_repeat(1);
    value.git_ref = "refs/heads/sea/integration".to_owned();
    value.workflow_ref =
        "owner/project/.github/workflows/qualification.yml@refs/heads/sea/integration".to_owned();
    value.campaign = repeat_campaign(&value);
    assert!(
        value
            .validate_for_config("main", Some("sea/integration"), "owner/project", SHA)
            .is_ok()
    );
    assert!(
        value
            .validate_for_config("main", Some("sea/integration"), "attacker/project", SHA)
            .is_err()
    );
    assert!(
        value
            .validate_for_config("main", None, "owner/project", SHA)
            .is_err()
    );
}

#[test]
fn shape_rejects_workflow_ref_for_a_different_runner_ref() {
    let mut value = dispatch();
    value.git_ref = "refs/heads/sea/integration".to_owned();
    assert!(value.validate_shape().is_err());
}

#[test]
fn rejects_unscoped_campaign_and_run_identity() {
    let mut value = dispatch();
    value.campaign = "../shared".to_owned();
    assert!(value.validate_shape().is_err());

    let mut value = dispatch();
    value.run_attempt = 0;
    assert!(value.validate_shape().is_err());
}

#[test]
fn phase_policy_keeps_third_read_only_and_control_disabled() {
    assert!(QualificationPhase::Cold.cache_enabled());
    assert!(QualificationPhase::Cold.cache_write_allowed());
    assert!(QualificationPhase::Warm.cache_write_allowed());
    assert!(QualificationPhase::Third.cache_enabled());
    assert!(!QualificationPhase::Third.cache_write_allowed());
    assert!(QualificationPhase::UsefulDelta.cache_write_allowed());
    assert!(!QualificationPhase::Control.cache_enabled());
    assert!(!QualificationPhase::Control.cache_write_allowed());
}

#[test]
fn predecessor_run_reference_is_required_only_for_lineage_phases() {
    for phase in [
        QualificationPhase::Cold,
        QualificationPhase::Warm,
        QualificationPhase::Third,
        QualificationPhase::UsefulDelta,
        QualificationPhase::Control,
    ] {
        let mut value = dispatch();
        value.phase = phase;
        assert_eq!(
            value.validate_shape().is_ok(),
            phase.predecessor().is_none(),
            "phase {phase:?} without predecessor"
        );
        if let Some(expected) = phase.predecessor() {
            value.predecessor = Some(QualificationRunRef {
                run_id: 121,
                run_attempt: 2,
            });
            assert!(value.validate_shape().is_ok(), "{phase:?} -> {expected:?}");
            value.predecessor = Some(QualificationRunRef {
                run_id: 0,
                run_attempt: 1,
            });
            assert!(value.validate_shape().is_err());
        } else {
            value.predecessor = Some(QualificationRunRef {
                run_id: 121,
                run_attempt: 1,
            });
            assert!(value.validate_shape().is_err(), "{phase:?}");
        }
    }
}
