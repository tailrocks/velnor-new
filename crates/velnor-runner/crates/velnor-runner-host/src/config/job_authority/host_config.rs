// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ApprovedJobAuthorityPolicy, ConnectionId};

use crate::error::HostError;

impl super::super::HostConfig {
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
}
