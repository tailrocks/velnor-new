//! Invalid source identities never reach git.

use super::impl_generator_source_scratch::Scratch;
use std::error::Error;
use std::fs;
use std::process::Command;
use velnor_actions_workflow_generator::generator_release::QUALIFICATION_SOURCE_PREPARE;

#[test]
fn reject_invalid_source_identity_before_running_git() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let workspace = scratch.0.join("workspace");
    fs::create_dir_all(&workspace)?;
    let log = scratch.0.join("git-arguments");
    let result = Command::new("bash")
        .arg("-c")
        .arg(QUALIFICATION_SOURCE_PREPARE)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_REPOSITORY", "attacker/other")
        .env("GITHUB_SHA", "not-a-commit")
        .env("MOCK_GIT_LOG", &log)
        .output()?;
    assert!(!result.status.success());
    assert!(!log.exists(), "Git ran for an invalid source identity");
    Ok(())
}
