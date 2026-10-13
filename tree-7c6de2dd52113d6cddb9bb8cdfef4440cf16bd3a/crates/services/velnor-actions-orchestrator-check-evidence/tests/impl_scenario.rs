//! Scenario evidence binds producer bytes to plan, check, and platform.

use velnor_actions_contract_config::config::{CheckEvidence, CheckPlatform};
use velnor_actions_orchestrator_check_evidence::scenario::{
    Scenario, ScenarioEvidence, ScenarioStatus, validate_evidence, verify_evidence,
};

fn declaration() -> CheckEvidence {
    CheckEvidence {
        path: "proof.json".into(),
        expected_scenarios: vec!["s1".into(), "s2".into()],
    }
}

fn evidence() -> ScenarioEvidence {
    ScenarioEvidence {
        schema: 1,
        source: "mise-task-v1".into(),
        head: "abc123".into(),
        check_id: "demo".into(),
        platform: CheckPlatform::LinuxX64,
        scenarios: vec![
            Scenario {
                id: "s1".into(),
                executed: true,
                status: ScenarioStatus::Passed,
            },
            Scenario {
                id: "s2".into(),
                executed: true,
                status: ScenarioStatus::Passed,
            },
        ],
    }
}

#[test]
fn validate_accepts_exact_evidence() {
    validate_evidence(
        &evidence(),
        &declaration(),
        "demo",
        "abc123",
        CheckPlatform::LinuxX64,
    )
    .expect("exact");
}

#[test]
fn validate_rejects_identity_drift() {
    for mode in 0..5 {
        let mut changed = evidence();
        match mode {
            0 => changed.schema = 2,
            1 => changed.source = "foreign".into(),
            2 => changed.head = "def456".into(),
            3 => changed.check_id = "other".into(),
            _ => changed.platform = CheckPlatform::MacosArm64,
        }
        assert!(
            validate_evidence(
                &changed,
                &declaration(),
                "demo",
                "abc123",
                CheckPlatform::LinuxX64,
            )
            .is_err(),
            "{mode}"
        );
    }
}

#[test]
fn validate_rejects_unexecuted_scenario() {
    let mut changed = evidence();
    changed.scenarios[0].executed = false;
    assert!(
        validate_evidence(
            &changed,
            &declaration(),
            "demo",
            "abc123",
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
}

#[test]
fn validate_rejects_failed_or_skipped_status() {
    for status in [ScenarioStatus::Failed, ScenarioStatus::Skipped] {
        let mut changed = evidence();
        changed.scenarios[1].status = status;
        assert!(
            validate_evidence(
                &changed,
                &declaration(),
                "demo",
                "abc123",
                CheckPlatform::LinuxX64,
            )
            .is_err()
        );
    }
}

#[test]
fn validate_rejects_missing_and_extra_scenarios() {
    let mut changed = evidence();
    changed.scenarios.pop();
    assert!(
        validate_evidence(
            &changed,
            &declaration(),
            "demo",
            "abc123",
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
    let mut changed = evidence();
    changed.scenarios.push(Scenario {
        id: "s3".into(),
        executed: true,
        status: ScenarioStatus::Passed,
    });
    assert!(
        validate_evidence(
            &changed,
            &declaration(),
            "demo",
            "abc123",
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
}

#[test]
fn verify_round_trip_binds_receipt_bytes() {
    let root = tempfile::TempDir::new().expect("root");
    let text = serde_json::to_string(&evidence()).expect("json");
    std::fs::write(root.path().join("proof.json"), &text).expect("write");
    let receipt = verify_evidence(
        root.path(),
        &declaration(),
        "demo",
        "abc123",
        CheckPlatform::LinuxX64,
    )
    .expect("receipt");
    assert_eq!(receipt.schema, 1);
    assert_eq!(receipt.check_id, "demo");
    assert_eq!(receipt.head, "abc123");
    assert_eq!(receipt.path, "proof.json");
    assert_eq!(receipt.bytes, text.as_bytes());
    assert_eq!(
        receipt.digest,
        velnor_actions_contract::digest_b3(text.as_bytes())
    );
}

#[test]
fn verify_missing_and_empty_files_fail() {
    let root = tempfile::TempDir::new().expect("root");
    assert!(
        verify_evidence(
            root.path(),
            &declaration(),
            "demo",
            "abc123",
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
    std::fs::write(root.path().join("proof.json"), "").expect("write");
    assert!(
        verify_evidence(
            root.path(),
            &declaration(),
            "demo",
            "abc123",
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
}
