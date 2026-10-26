// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;
use std::{fs, io};

use velnor_actions_orchestrator::{OrchestratorError, prepare};

use super::workflow_configuration_fixture;

#[test]
fn prepare_preserves_default_and_optional_verification_branches() -> Result<(), Box<dyn Error>> {
    let cases = [
        (None, None),
        (Some("sea/integration"), Some("sea/integration")),
    ];
    for (configured_branch, expected_branch) in cases {
        let config =
            workflow_configuration_fixture::config_with_verification_branch(configured_branch);
        let repo = workflow_configuration_fixture::make_repo(&config)?;
        let prepared = prepare(repo.path())?;
        assert_eq!(prepared.default_branch, "testmain");
        assert_eq!(
            prepared.config.workflow.default_branch.as_deref(),
            Some("testmain")
        );
        assert_eq!(
            prepared.config.workflow.verification_branch.as_deref(),
            expected_branch
        );
    }
    Ok(())
}

#[test]
fn prepare_rejects_equal_and_malformed_verification_branches() -> Result<(), Box<dyn Error>> {
    let cases = [
        (Some("testmain"), "must_differ_from_default_branch"),
        (Some("sea/../main"), "malformed_branch"),
    ];
    for (configured_branch, expected_problem) in cases {
        let config =
            workflow_configuration_fixture::config_with_verification_branch(configured_branch);
        let repo = workflow_configuration_fixture::make_repo(&config)?;
        let Err(error) = prepare(repo.path()) else {
            return Err(io::Error::other(format!(
                "verification branch {configured_branch:?} unexpectedly prepared"
            ))
            .into());
        };
        match error {
            OrchestratorError::Config {
                file,
                key_path,
                problem,
            } => {
                assert_eq!(file, ".velnor/config.toml");
                assert_eq!(key_path, "workflow.verification_branch");
                assert_eq!(problem, expected_problem);
            }
            other => {
                return Err(io::Error::other(format!(
                    "unexpected error for verification branch {configured_branch:?}: {other}"
                ))
                .into());
            }
        }
    }
    Ok(())
}

#[test]
fn prepare_resolves_verification_branch_against_origin_default() -> Result<(), Box<dyn Error>> {
    let repo =
        workflow_configuration_fixture::make_repo("schema = 1\n[workflow]\nname = \"CI\"\n")?;
    let root = repo.path();
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    workflow_configuration_fixture::git(&["commit", "-m", "fixture"], root)?;
    workflow_configuration_fixture::git(
        &["update-ref", "refs/remotes/origin/integration-main", "HEAD"],
        root,
    )?;
    workflow_configuration_fixture::git(
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/integration-main",
        ],
        root,
    )?;

    for (configured_branch, expected_branch) in
        [(None, None), (Some("sea/verify"), Some("sea/verify"))]
    {
        write_verification_config(root, configured_branch)?;
        let prepared = prepare(root)?;
        assert_eq!(prepared.default_branch, "integration-main");
        assert_eq!(prepared.config.workflow.default_branch, None);
        assert_eq!(
            prepared.config.workflow.verification_branch.as_deref(),
            expected_branch
        );
    }

    write_verification_config(root, Some("integration-main"))?;
    let Err(error) = prepare(root) else {
        return Err(io::Error::other(
            "verification branch equal to the origin default unexpectedly prepared",
        )
        .into());
    };
    match error {
        OrchestratorError::Config {
            file,
            key_path,
            problem,
        } => {
            assert_eq!(file, ".velnor/config.toml");
            assert_eq!(key_path, "workflow.verification_branch");
            assert_eq!(problem, "must_differ_from_default_branch");
        }
        other => {
            return Err(
                io::Error::other(format!("unexpected effective-default error: {other}")).into(),
            );
        }
    }
    Ok(())
}

fn write_verification_config(root: &Path, branch: Option<&str>) -> Result<(), Box<dyn Error>> {
    let verification_branch = branch.map_or_else(String::new, |branch| {
        format!("verification_branch = \"{branch}\"\n")
    });
    let config = format!("schema = 1\n[workflow]\nname = \"CI\"\n{verification_branch}");
    fs::write(root.join(".velnor/config.toml"), config)?;
    Ok(())
}
