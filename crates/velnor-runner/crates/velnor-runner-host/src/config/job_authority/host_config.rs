#[cfg(test)]
use super::{
    ApprovedEventContext, ApprovedWorkflow, GeneratorIdentity, JobAuthorityRole, POLICY_SCHEMA,
    PermissionLevel, WorkflowEvent, sha256,
};
use super::{
    ApprovedJobAuthorityEntry, ApprovedJobAuthorityPolicy, ConnectionId, JobAuthorityLookup,
};
#[cfg(test)]
use crate::config::RepositoryConnection;
use crate::error::HostError;

impl super::super::HostConfig {
    /// Add one reviewed authority policy from JSON.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the JSON does not add one valid policy for the named
    /// connection or changes any existing configuration.
    pub fn with_job_authority_policy_json(
        &self,
        connection_id: &ConnectionId,
        policy_json: &str,
    ) -> Result<Self, HostError> {
        const MAX_POLICY_BYTES: usize = 256 * 1024;
        self.validate()?;
        if policy_json.is_empty()
            || policy_json.len() > MAX_POLICY_BYTES
            || !self
                .connections
                .iter()
                .any(|item| &item.connection_id == connection_id)
            || self
                .job_authority_policies
                .iter()
                .any(|policy| policy.connection_id() == connection_id)
        {
            return Err(HostError::Config);
        }

        let mut policy: ApprovedJobAuthorityPolicy =
            serde_json::from_str(policy_json).map_err(|_| HostError::Config)?;
        policy.canonicalize();
        if policy.connection_id() != connection_id {
            return Err(HostError::Config);
        }
        let mut candidate = self.clone();
        candidate.job_authority_policies.push(policy);
        candidate.validate()?;
        Ok(candidate)
    }

    /// Add one policy built from a validated generator inventory and reviewed identities.
    ///
    /// The generator identity values are operator-reviewed provenance. This method does not
    /// authenticate the release that produced the inventory.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the inventory, identities, connection, or resulting
    /// policy is invalid.
    pub fn with_job_authority_inventory(
        &self,
        connection_id: &ConnectionId,
        inventory_json: &str,
        generator_source_sha: &str,
        generator_artifact_sha256: &str,
        generation: u64,
    ) -> Result<Self, HostError> {
        let connection = self
            .connections
            .iter()
            .find(|connection| &connection.connection_id == connection_id)
            .ok_or(HostError::Config)?;
        let policy_json = super::inventory_bridge::policy_json_from_inventory(
            connection,
            inventory_json,
            generator_source_sha,
            generator_artifact_sha256,
            generation,
        )?;
        self.with_job_authority_policy_json(connection_id, &policy_json)
    }

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

    /// Add one valid test-only authority policy through normal `HostConfig` validation.
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
