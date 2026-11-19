// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

pub(super) async fn recover(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "UPDATE connection_execution_proofs SET challenge_state = 'requested' WHERE challenge_state = 'not_requested' AND proof_state = 'pending'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_execution_proofs SET challenge_state = 'uncertain', proof_state = 'denied', deny_code = 'interrupted_after_restart' WHERE challenge_state = 'requested' AND proof_state = 'pending'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_execution_proofs SET challenge_state = 'abandoned', proof_state = 'denied', deny_code = 'interrupted_after_restart' WHERE challenge_state = 'published' AND proof_state = 'pending'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_execution_proofs SET proof_state = 'denied', deny_code = 'interrupted_after_restart' WHERE challenge_state IN ('uncertain', 'abandoned') AND proof_state = 'pending'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_execution_decisions SET delivery_state = 'abandoned' WHERE delivery_state = 'prepared'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_execution_decisions SET delivery_state = 'uncertain' WHERE delivery_state = 'requested'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
