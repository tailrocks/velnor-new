//! Named scenario proof rejects empty, skipped, stale, and foreign reports.
use crate::check_evidence::{Scenario, ScenarioEvidence, ScenarioStatus, validate_evidence};
use velnor_actions_contract::config::{CheckEvidence, CheckPlatform};

fn declaration() -> CheckEvidence {
    CheckEvidence {
        path: "proof.json".into(),
        expected_scenarios: vec!["one".into(), "two".into()],
    }
}
fn proof() -> ScenarioEvidence {
    ScenarioEvidence {
        schema: 1,
        source: "mise-task-v1".into(),
        head: "abc".into(),
        check_id: "docker".into(),
        platform: CheckPlatform::MacosArm64,
        scenarios: vec!["one", "two"]
            .into_iter()
            .map(|id| Scenario {
                id: id.into(),
                executed: true,
                status: ScenarioStatus::Passed,
            })
            .collect(),
    }
}
fn valid(p: &ScenarioEvidence, d: &CheckEvidence) -> bool {
    validate_evidence(p, d, "docker", "abc", CheckPlatform::MacosArm64).is_ok()
}
#[test]
fn exact_passed_scenarios_required() {
    assert!(valid(&proof(), &declaration()));
    let mut p = proof();
    p.scenarios.clear();
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.scenarios.pop();
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.scenarios[1].id = "one".into();
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.scenarios[1].id = "foreign".into();
    assert!(!valid(&p, &declaration()));
    let mut d = declaration();
    d.expected_scenarios.clear();
    assert!(!valid(&proof(), &d));
}
#[test]
fn skipped_failed_unexecuted_and_stale_rejected() {
    let mut p = proof();
    p.scenarios[0].executed = false;
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.scenarios[0].status = ScenarioStatus::Skipped;
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.scenarios[0].status = ScenarioStatus::Failed;
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.head = "old".into();
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.check_id = "foreign".into();
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.source = "other".into();
    assert!(!valid(&p, &declaration()));
    let mut p = proof();
    p.platform = CheckPlatform::LinuxX64;
    assert!(!valid(&p, &declaration()));
}
#[test]
fn evidence_files_missing_empty_and_symlink_refuse() {
    let root = tempfile::TempDir::new().expect("temp");
    let verify = || {
        crate::check_evidence::verify_evidence(
            root.path(),
            &declaration(),
            "docker",
            "abc",
            CheckPlatform::MacosArm64,
        )
    };
    assert!(verify().is_err());
    std::fs::write(root.path().join("proof.json"), "").expect("write");
    assert!(verify().is_err());
    std::fs::write(
        root.path().join("proof.json"),
        serde_json::to_vec(&proof()).expect("json"),
    )
    .expect("write");
    let receipt = verify().expect("valid proof");
    assert!(receipt.digest.starts_with("b3-"));
    #[cfg(unix)]
    {
        std::fs::remove_file(root.path().join("proof.json")).expect("remove");
        std::fs::write(root.path().join("actual.json"), "{}").expect("write");
        std::os::unix::fs::symlink("actual.json", root.path().join("proof.json")).expect("link");
        assert!(verify().is_err());
    }
}
#[test]
fn strict_evidence_parser_rejects_duplicate_and_foreign_fields() {
    let root = tempfile::TempDir::new().expect("temp");
    let json = serde_json::to_string(&proof()).expect("json");
    for field in [",\"schema\":1", ",\"foreign\":true"] {
        let mut bad = json.clone();
        bad.pop();
        bad.push_str(field);
        bad.push('}');
        std::fs::write(root.path().join("proof.json"), bad).expect("write");
        assert!(
            crate::check_evidence::verify_evidence(
                root.path(),
                &declaration(),
                "docker",
                "abc",
                CheckPlatform::MacosArm64
            )
            .is_err()
        );
    }
}

#[cfg(unix)]
#[test]
fn parent_symlinks_refused_even_inside_repository() {
    let root = tempfile::TempDir::new().expect("temp");
    std::fs::create_dir(root.path().join("actual")).expect("dir");
    std::os::unix::fs::symlink("actual", root.path().join("alias")).expect("link");
    assert!(
        crate::check_evidence::reject_link_components(root.path(), "alias/proof.json").is_err()
    );
}
