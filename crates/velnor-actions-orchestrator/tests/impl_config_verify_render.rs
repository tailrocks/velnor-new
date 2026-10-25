//! The retired Zizmor verifier is rejected by the current contract.

use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

#[test]
fn configured_zizmor_is_rejected() -> TestResult {
    let config = format!(
        "{}\n[workflow.verify]\njobs = [\"zizmor\"]\n",
        config_with_branch()
    );
    let repo = make_repo(&config)?;
    let error = prepare(repo.path())
        .err()
        .ok_or("the retired Zizmor validator must be rejected")?;
    assert!(
        error
            .to_string()
            .contains("zizmor_retired:jactionlint covers the zizmor audits"),
        "unexpected config error: {error}"
    );
    Ok(())
}
