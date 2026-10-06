//! Independently observed runner authority; staged requests cannot choose it.

use super::{OrchestratorError, PublishRequest, internal};

pub(super) struct RunnerPublicationAuthority {
    event: Option<String>,
    protected: Option<String>,
    repository: Option<String>,
    git_ref: Option<String>,
    head: Option<String>,
    workflow_sha: Option<String>,
    workflow_ref: Option<String>,
    default_branch: Option<String>,
}

impl RunnerPublicationAuthority {
    pub(super) fn capture() -> Result<Self, OrchestratorError> {
        let path = std::env::var_os("GITHUB_EVENT_PATH")
            .ok_or_else(|| internal("publish_refused:missing_runner_payload"))?;
        let text = crate::safe_read::read_event_file(
            std::path::Path::new(&path),
            crate::safe_read::MAX_REPO_FILE_BYTES,
        )?;
        let payload = velnor_actions_contract::parse_strict_json(&text)
            .map_err(|_| internal("publish_refused:malformed_runner_payload"))?;
        let branch = payload["repository"]["default_branch"]
            .as_str()
            .map(str::to_owned);
        Ok(Self::from_reader(|name| std::env::var(name).ok(), branch))
    }

    fn from_reader(read: impl Fn(&str) -> Option<String>, default_branch: Option<String>) -> Self {
        Self {
            event: read("GITHUB_EVENT_NAME"),
            protected: read("GITHUB_REF_PROTECTED"),
            repository: read("GITHUB_REPOSITORY"),
            git_ref: read("GITHUB_REF"),
            head: read("GITHUB_SHA"),
            workflow_sha: read("GITHUB_WORKFLOW_SHA"),
            workflow_ref: read("GITHUB_WORKFLOW_REF"),
            default_branch,
        }
    }

    pub(super) fn verify(&self, request: &PublishRequest) -> Result<(), OrchestratorError> {
        if self.event.as_deref() != Some("push") || self.protected.as_deref() != Some("true") {
            return Err(internal("publish_refused:runner_unprotected"));
        }
        let (Some(repository), Some(git_ref)) = (&self.repository, &self.git_ref) else {
            return Err(internal("publish_refused:runner_source_unanchored"));
        };
        let normalize = crate::origin::validate_repository_slug;
        let repository = normalize(repository);
        let suffix = format!(
            "/{}@{git_ref}",
            velnor_actions_workflow_renderer::WORKFLOW_PATH
        );
        let workflow_repository = self
            .workflow_ref
            .as_deref()
            .and_then(|value| value.strip_suffix(&suffix))
            .and_then(normalize);
        let default_ref = self
            .default_branch
            .as_deref()
            .filter(|branch| velnor_actions_contract::is_valid_branch_name(branch))
            .map(|branch| format!("refs/heads/{branch}"));
        if repository.is_none()
            || request.repository.as_deref().and_then(normalize) != repository
            || request.git_ref.as_ref() != Some(git_ref)
            || default_ref.as_ref() != Some(git_ref)
            || request.default_branch != self.default_branch
            || self.head.as_ref() != Some(&request.head)
            || self.workflow_sha.as_ref() != Some(&request.head)
            || workflow_repository != repository
        {
            return Err(internal("publish_refused:runner_source_mismatch"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "baseline_publish_authority_tests.rs"]
mod tests;
