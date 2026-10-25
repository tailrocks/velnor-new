use super::dispatch_is_admitted;
use velnor_actions_contract::{QualificationDispatch, QualificationPhase};

const REPOSITORY: &str = "owner/tap";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn dispatch() -> QualificationDispatch {
    QualificationDispatch {
        campaign: "qualification".to_owned(),
        phase: QualificationPhase::Cold,
        repository: REPOSITORY.to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
        workflow_sha: SHA.to_owned(),
        source_sha: SHA.to_owned(),
        run_id: 42,
        run_attempt: 1,
        predecessor: None,
    }
}

#[test]
fn qualification_dispatch_is_confined_to_protected_main_and_exact_ci_source() {
    let accepted = dispatch();
    assert!(dispatch_is_admitted(
        &accepted,
        REPOSITORY,
        SHA,
        "workflow_dispatch"
    ));

    let mut rejected = dispatch();
    rejected.git_ref = "refs/tags/main".to_owned();
    assert!(!dispatch_is_admitted(
        &rejected,
        REPOSITORY,
        SHA,
        "workflow_dispatch"
    ));
    rejected = dispatch();
    rejected.ref_protected = false;
    assert!(!dispatch_is_admitted(
        &rejected,
        REPOSITORY,
        SHA,
        "workflow_dispatch"
    ));
    rejected = dispatch();
    rejected.workflow_sha = "b".repeat(40);
    assert!(!dispatch_is_admitted(
        &rejected,
        REPOSITORY,
        SHA,
        "workflow_dispatch"
    ));
    assert!(!dispatch_is_admitted(&accepted, REPOSITORY, SHA, "push"));
}
