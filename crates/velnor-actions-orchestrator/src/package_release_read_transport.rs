//! Bounded JSON reads shared by release transport and exact-source metadata.
use super::{GitHubApi, JSON_LIMIT, RepositoryResponse};
use crate::{OrchestratorError, internal::internal};

impl GitHubApi {
    pub(in crate::package_release_publish) fn verify_repository_identity(
        &self,
        expected_id: u64,
    ) -> Result<(), OrchestratorError> {
        let repository: RepositoryResponse =
            self.get_json(&format!("repos/{}", self.repository))?;
        if repository.id != expected_id
            || repository.full_name.as_deref() != Some(self.repository.as_str())
        {
            return Err(internal("package_publish_repository_mismatch"));
        }
        Ok(())
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
