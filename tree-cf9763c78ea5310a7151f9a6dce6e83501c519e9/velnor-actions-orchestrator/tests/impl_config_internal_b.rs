//! Init/config follow-on cases, split from `impl_config_internal.rs`
//! to satisfy the 400-line gate (rust-quality-contract §5).
use std::fs;

use velnor_actions_orchestrator::{OrchestratorError, finalized_jobs, plan_text, prepare};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, make_repo, without_ambient_identity,
};

#[test]
fn branch_failure_hints_default_branch_setting() -> TestResult {
    let repo = make_repo("schema = 1\n")?;
    let err = err_of(prepare(repo.path()), "branch unresolvable")?;
    assert!(
        matches!(err, OrchestratorError::DefaultBranch { .. }),
        "got {err}"
    );
    assert!(
        err.to_string().contains("set workflow.default_branch"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn plan_job_lines_come_from_finalized_jobs() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let jobs = finalized_jobs(&prep)?;
    let text = plan_text(&prep, &jobs);
    assert!(!jobs.is_empty(), "finalized jobs exist");
    for (id, job) in &jobs {
        let line = format!("- {id} ({} steps)", job.steps.len());
        assert!(text.contains(&line), "missing {line}:\n{text}");
    }
    assert!(text.contains("1 Rust crate job"), "crate detail:\n{text}");
    Ok(())
}

#[test]
fn identity_ignores_decoy_lines_and_remotes() -> TestResult {
    without_ambient_identity("identity_ignores_decoy_lines_and_remotes", || {
        let config = "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n";
        // Decoy: identity in another remote, in a non-url key, and in comments.
        let repo = make_repo(config)?;
        let git_config = repo.path().join(".git/config");
        let mut text = fs::read_to_string(&git_config)?;
        text.push_str(
        "[remote \"upstream\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n[remote \"origin\"]\n\turl = https://example.com/other/repo.git\n\tpushurl = https://github.com/tailrocks/velnor-new.git\n# tailrocks/velnor-new\n",
    );
        fs::write(&git_config, text)?;
        let err = err_of(prepare(repo.path()), "decoys rejected")?;
        assert!(
            matches!(err, OrchestratorError::IdentityRejected { .. }),
            "got {err}"
        );
        // Positive control: origin url grants the identity.
        let repo = make_repo(config)?;
        let git_config = repo.path().join(".git/config");
        let mut text = fs::read_to_string(&git_config)?;
        text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
        fs::write(&git_config, text)?;
        prepare(repo.path())?;
        Ok(())
    })
}
