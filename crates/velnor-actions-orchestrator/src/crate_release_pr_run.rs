// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) struct Context {
    repository: String,
    repository_id: u64,
    branch: String,
    sha: String,
    run_id: u64,
    artifact_id: u64,
    archive_digest: String,
    content_digest: String,
    token: String,
    runner_temp: PathBuf,
}

fn positive_number(value: &str) -> Result<u64, OrchestratorError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| internal("release_pr_environment_invalid"))
}

fn hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Context {
    pub(crate) fn from_values(
        read: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, OrchestratorError> {
        let required = |name: &str| {
            read(name)
                .filter(|value| !value.is_empty() && !value.bytes().any(|b| b.is_ascii_control()))
                .ok_or_else(|| internal("release_pr_environment_invalid"))
        };
        let invalid = || internal("release_pr_environment_invalid");
        let repository = required("GITHUB_REPOSITORY")?;
        let (owner, repo) = repository.split_once('/').ok_or_else(invalid)?;
        if !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(repo)
            || required("GITHUB_EVENT_NAME")? != "push"
            || required("GITHUB_RUN_ATTEMPT")? != "1"
        {
            return Err(invalid());
        }
        let reference = required("GITHUB_REF")?;
        let branch = reference
            .strip_prefix("refs/heads/")
            .filter(|branch| velnor_actions_contract::is_valid_branch_name(branch))
            .ok_or_else(invalid)?
            .to_owned();
        let sha = required("GITHUB_SHA")?;
        let archive_digest = required("VELNOR_PR_ARTIFACT_DIGEST")?;
        let content_digest = required("VELNOR_PR_CONTENT_SHA256")?;
        if !hex(&sha, 40)
            || required("GITHUB_WORKFLOW_SHA")? != sha
            || required("GITHUB_WORKFLOW_REF")?
                != format!(
                    "{repository}/{}@{reference}",
                    velnor_actions_contract::CRATE_RELEASE_WORKFLOW_PATH
                )
            || !hex(&archive_digest, 64)
            || !hex(&content_digest, 64)
        {
            return Err(invalid());
        }
        let runner_temp = PathBuf::from(required("RUNNER_TEMP")?);
        if !runner_temp.is_absolute()
            || !runner_temp.is_dir()
            || runner_temp.canonicalize().map_err(|_| invalid())? != runner_temp
        {
            return Err(invalid());
        }
        Ok(Self {
            repository,
            repository_id: positive_number(&required("GITHUB_REPOSITORY_ID")?)?,
            branch,
            sha,
            run_id: positive_number(&required("GITHUB_RUN_ID")?)?,
            artifact_id: positive_number(&required("VELNOR_PR_ARTIFACT_ID")?)?,
            archive_digest,
            content_digest,
            token: required("GIT_TOKEN")?,
            runner_temp,
        })
    }
}

pub(crate) fn run_from_environment() -> Result<(), OrchestratorError> {
    let context = Context::from_values(|name| env::var(name).ok())?;
    let github =
        crate::crate_release_github::NativeGitHubAdmissionRead::new(context.token.clone())?;
    run_with(
        &context,
        &github,
        &crate::crate_release_tool_download::NativeToolDownloadTransport,
    )
}

pub(crate) fn run_with<T: crate::crate_release_tool_download::ToolDownloadTransport>(
    context: &Context,
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
    transport: &T,
) -> Result<(), OrchestratorError> {
    let request = crate::release::crate_release_pr_acquisition::AcquisitionRequest {
        binding: crate::release::crate_release_pr_acquisition::RunBinding {
            repository: &context.repository,
            branch: &context.branch,
            sha: &context.sha,
            run_id: context.run_id,
            attempt: 1,
        },
        artifact_id: context.artifact_id,
        archive_digest: &context.archive_digest,
        content_digest: &context.content_digest,
        token: &context.token,
        runner_temp: &context.runner_temp,
        deadline: Instant::now() + Duration::from_secs(180),
    };
    let preparation = crate::release::crate_release_pr_acquisition::acquire_preparation(
        &request, github, transport,
    )?;
    if preparation.repository_id != context.repository_id {
        return Err(internal("release_pr_environment_invalid"));
    }
    crate::release::crate_release_pr_writer::write_prepared_pr(github, &preparation)
}

#[cfg(test)]
mod tests {
    use super::Context;
    use std::{collections::BTreeMap, fs};

    #[test]
    fn accepts_only_exact_push_runner_context() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let sha = "a".repeat(40);
        let mut values = BTreeMap::from([
            ("GITHUB_REPOSITORY", "owner/project".to_owned()),
            ("GITHUB_REPOSITORY_ID", "7".to_owned()),
            ("GITHUB_EVENT_NAME", "push".to_owned()),
            ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
            ("GITHUB_REF", "refs/heads/main".to_owned()),
            ("GITHUB_SHA", sha.clone()),
            ("GITHUB_WORKFLOW_SHA", sha),
            (
                "GITHUB_WORKFLOW_REF",
                "owner/project/.github/workflows/release.yml@refs/heads/main".to_owned(),
            ),
            ("GITHUB_RUN_ID", "42".to_owned()),
            ("VELNOR_PR_ARTIFACT_ID", "51".to_owned()),
            ("VELNOR_PR_ARTIFACT_DIGEST", "b".repeat(64)),
            ("VELNOR_PR_CONTENT_SHA256", "c".repeat(64)),
            ("GIT_TOKEN", "test-credential".to_owned()),
            ("RUNNER_TEMP", root.to_string_lossy().into_owned()),
        ]);
        let check =
            |items: &BTreeMap<&str, String>| Context::from_values(|name| items.get(name).cloned());
        let context = check(&values)?;
        assert_eq!(context.repository_id, 7);
        assert_eq!(context.branch, "main");
        for (key, invalid) in [
            ("GITHUB_EVENT_NAME", "workflow_dispatch"),
            ("GITHUB_RUN_ATTEMPT", "2"),
            ("GITHUB_REF", "refs/tags/v1"),
            ("GITHUB_WORKFLOW_SHA", "b"),
            (
                "GITHUB_WORKFLOW_REF",
                "owner/other/.github/workflows/release.yml@refs/heads/main",
            ),
            ("GITHUB_REPOSITORY_ID", "0"),
            ("GITHUB_RUN_ID", "0"),
            ("VELNOR_PR_ARTIFACT_ID", "-1"),
            ("VELNOR_PR_ARTIFACT_DIGEST", "sha256:bad"),
            ("VELNOR_PR_CONTENT_SHA256", "bad"),
            ("GIT_TOKEN", ""),
        ] {
            let old = values
                .insert(key, invalid.to_owned())
                .ok_or("missing key")?;
            assert!(check(&values).is_err(), "accepted invalid {key}");
            values.insert(key, old);
        }
        #[cfg(unix)]
        {
            let link = root.join("link");
            std::os::unix::fs::symlink(&root, &link)?;
            values.insert("RUNNER_TEMP", link.to_string_lossy().into_owned());
            assert!(check(&values).is_err());
            fs::remove_file(link)?;
        }
        Ok(())
    }
}
