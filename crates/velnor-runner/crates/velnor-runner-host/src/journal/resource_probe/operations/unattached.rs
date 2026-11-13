// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::in_transaction;

use super::hex_string;

impl Journal {
    pub(crate) async fn abort_unattached_probe(
        &self,
        operation_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<(), HostError> {
        if !hex_string(operation_id, 32) || !hex_string(docker_root_digest, 64) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let aborted = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'aborted' WHERE operation_id = ?1 AND helper_role = 'resource-probe' AND operation_name = 'velnor-resource-probe-' || operation_id AND engine_id = ?2 AND docker_root_digest = ?3 AND phase IN ('create_requested', 'quarantined') AND container_id IS NULL",
                    (operation_id, engine_id, docker_root_digest),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if aborted != 1 {
                return Err(HostError::Journal);
            }
            let released = connection
                .execute(
                    "UPDATE host_operation_leases SET state = 'released' WHERE singleton = 1 AND operation_id = ?1 AND engine_id = ?2 AND docker_root_digest = ?3 AND container_id IS NULL AND state IN ('active', 'uncertain', 'recovering')",
                    (operation_id, engine_id, docker_root_digest),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if released == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }
}
