use super::{
    ApprovedJobAuthorityEntry, ConnectionId, Deserialize, Digest, JobAuthorityLookup, Serialize,
};
use crate::error::HostError;

impl super::super::HostConfig {
    /// Build a parsed test config with one validator-approved policy for a real binding.
    #[cfg(test)]
    pub(crate) fn test_with_job_authority_policy(
        connection: &RepositoryConnection,
    ) -> Result<Self, HostError> {
        Self::test_with_job_authority_policy_generation(connection, 1)
    }

    #[cfg(test)]
    pub(crate) fn test_with_job_authority_policy_generation(
        connection: &RepositoryConnection,
        generation: u64,
    ) -> Result<Self, HostError> {
        let text = crate::config_test_support::host_config(
            &[crate::config_test_support::basic_connection(
                "test-policy",
                1,
                "test/repository",
                "test-policy",
                "test-set",
            )],
            1,
        );
        let mut config = Self::parse(&text)?;
        config.connections = vec![connection.clone()];
        config.add_test_job_authority_policy_generation(&connection.connection_id, generation)?;
        Ok(config)
    }

    /// Add one valid test-only authority policy through normal HostConfig validation.
    #[cfg(test)]
    pub(crate) fn add_test_job_authority_policy(
        &mut self,
        connection_id: &super::ConnectionId,
    ) -> Result<(), HostError> {
        self.add_test_job_authority_policy_generation(connection_id, 1)
    }

    #[cfg(test)]
    pub(crate) fn add_test_job_authority_policy_generation(
        &mut self,
        connection_id: &super::ConnectionId,
        generation: u64,
    ) -> Result<(), HostError> {
        use std::collections::BTreeMap;

        if generation == 0 {
            return Err(HostError::Config);
        }

        let connection = self
            .connections
            .iter()
            .find(|connection| &connection.connection_id == connection_id)
            .ok_or(HostError::Config)?;
        let path = ".github/workflows/build.yml";
        let mut policy = ApprovedJobAuthorityPolicy {
            schema_version: POLICY_SCHEMA,
            connection_id: connection.connection_id.clone(),
            binding_sha256: sha256(&connection.canonical_identity_bytes()?),
            generation,
            policy_sha256: String::new(),
            generator: GeneratorIdentity {
                source_sha: "a".repeat(40),
                artifact_sha256: "b".repeat(64),
            },
            workflows: vec![ApprovedWorkflow {
                repository_id: connection.repository_id,
                path: path.to_owned(),
                content_sha256: "c".repeat(64),
                calls: Vec::new(),
                jobs: vec![ApprovedJobAuthorityEntry {
                    root_path: path.to_owned(),
                    workflow_path: path.to_owned(),
                    job_key: "required".to_owned(),
                    role: JobAuthorityRole::Verification,
                    contexts: vec![ApprovedEventContext {
                        event: WorkflowEvent::Push,
                        refs: vec!["refs/heads/sea/integration".to_owned()],
                        pull_request_rules: Vec::new(),
                    }],
                    permissions_declared: true,
                    permissions: BTreeMap::from([("contents".to_owned(), PermissionLevel::Read)]),
                    secrets: Vec::new(),
                    environments: Vec::new(),
                    condition_sha256: None,
                    concurrency: None,
                    needs: Vec::new(),
                }],
            }],
        };
        policy.canonicalize();
        policy.policy_sha256 =
            sha256(&serde_json::to_vec(&policy.content()).map_err(|_| HostError::Config)?);
        self.job_authority_policies.push(policy);
        self.validate()
    }

    /// Remove one repository binding and its approved job policy.
    ///
    /// An empty connection list is a disconnected state. Callers must not start
    /// repository listeners until the list contains a connection.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the current config is invalid.
    pub fn remove_connection(&mut self, id: &ConnectionId) -> Result<bool, HostError> {
        self.validate()?;
        let Some(index) = self
            .connections
            .iter()
            .position(|connection| &connection.connection_id == id)
        else {
            return Ok(false);
        };
        let connection = self.connections.remove(index);
        let policy_index = self
            .job_authority_policies
            .iter()
            .position(|policy| policy.connection_id() == id);
        let policy =
            policy_index.map(|policy_index| self.job_authority_policies.remove(policy_index));
        if let Err(error) = self.validate() {
            self.connections.insert(index, connection);
            if let (Some(policy_index), Some(policy)) = (policy_index, policy) {
                self.job_authority_policies.insert(policy_index, policy);
            }
            return Err(error);
        }
        Ok(true)
    }

    /// Find one approved job entry for the current policy generation.
    /// Use a merge-group target ref only after payload authentication.
    #[must_use]
    pub(crate) fn approved_job_authority(
        &self,
        lookup: &JobAuthorityLookup<'_>,
    ) -> Option<&ApprovedJobAuthorityEntry> {
        self.job_authority_policies
            .iter()
            .find(|policy| {
                policy.connection_id() == lookup.connection_id
                    && policy.generation() == lookup.generation
            })?
            .entry(lookup)
    }
}
