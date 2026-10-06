use super::{QualificationDispatch, QualificationPhase, QualificationRunRef};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn dispatch() -> QualificationDispatch {
    QualificationDispatch {
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
    }
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
