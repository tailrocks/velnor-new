//! Bounded JSON reads shared by release transport and exact-source metadata.
use super::{GitHubApi, JSON_LIMIT, RepositoryResponse};
use crate::{OrchestratorError, internal::internal};

impl GitHubApi {
    pub(in crate::package_release_publish) fn verify_repository_identity(
        &self,
        expected_id: u64,
    ) -> Result<(), OrchestratorError> {
        let repository = self.repository_identity()?;
        if repository.id != expected_id
            || repository.full_name.as_deref() != Some(self.repository.as_str())
        {
            return Err(internal("package_publish_repository_mismatch"));
        }
        Ok(())
    }

    pub(in crate::package_release_publish) fn verify_preview_repository_identity(
        &self,
        expected_id: u64,
        expected_default_branch: &str,
    ) -> Result<(), OrchestratorError> {
        let repository = self.repository_identity()?;
        if repository.id != expected_id
            || repository.full_name.as_deref() != Some(self.repository.as_str())
        {
            return Err(internal("package_publish_repository_mismatch"));
        }
        if repository.default_branch.as_deref() != Some(expected_default_branch) {
            return Err(internal("package_publish_default_branch_mismatch"));
        }
        Ok(())
    }

    fn repository_identity(&self) -> Result<RepositoryResponse, OrchestratorError> {
        self.get_json(&format!("repos/{}", self.repository))
    }

    pub(in crate::package_release_publish) fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        route: &str,
    ) -> Result<T, OrchestratorError> {
        self.get_json_bounded(route, JSON_LIMIT)
    }

    pub(in crate::package_release_publish) fn get_json_bounded<T: serde::de::DeserializeOwned>(
        &self,
        route: &str,
        limit: usize,
    ) -> Result<T, OrchestratorError> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return serde_json::from_value(fixture.read(route)?)
                .map_err(|_| internal("package_publish_api_json"));
        }
        let bytes = self.request(
            crate::github_authority_read::arguments(route)?,
            limit.min(8 * 1024 * 1024),
        )?;
        serde_json::from_slice(&bytes).map_err(|_| internal("package_publish_api_json"))
    }
}
