//! Claim-fenced container bindings and final cleanup proof.

use crate::error::HostError;
use crate::journal::{Journal, token_rejected};

use super::claim::{current_claim, unix_seconds};
use super::finish_transaction;

impl Journal {
    /// Bind owned container ids only while this completion generation is current.
    pub(crate) async fn bind_completion_containers(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
    ) -> Result<bool, HostError> {
        if id <= 0
            || generation <= 0
            || runner_container_id.is_some_and(token_rejected)
            || dind_id.is_some_and(token_rejected)
        {
            return Err(HostError::Journal);
        }
        self.bind_completion_containers_with_clock(
            id,
            generation,
            runner_container_id,
            dind_id,
            unix_seconds,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn bind_completion_containers_at(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0
            || generation <= 0
            || now < 0
            || runner_container_id.is_some_and(token_rejected)
            || dind_id.is_some_and(token_rejected)
        {
            return Err(HostError::Journal);
        }
        self.bind_completion_containers_with_clock(
            id,
            generation,
            runner_container_id,
            dind_id,
            || Ok(now),
        )
        .await
    }

    async fn bind_completion_containers_with_clock(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 || !current_claim(&connection, id, generation, now).await? {
                return Ok(false);
            }
            if runner_container_id.is_none() && dind_id.is_none() {
                return Ok(true);
            }
            bind_ids(&connection, id, runner_container_id, dind_id).await
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Persist exact official runner absence under the current cleanup claim.
    pub(crate) async fn record_completion_runner_absent(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        self.record_completion_runner_absent_with_clock(id, generation, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn record_completion_runner_absent_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        self.record_completion_runner_absent_with_clock(id, generation, || Ok(now))
            .await
    }

    async fn record_completion_runner_absent_with_clock(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE completion_cleanup SET runner_absent = 1 WHERE intent_id = ?1 AND claim_generation = ?2 AND lease_until > ?3 AND EXISTS (SELECT 1 FROM intents WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0)",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Commit final proof only for the current unexpired generation.
    pub(crate) async fn record_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        self.record_completion_cleanup_with_clock(id, generation, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn record_completion_cleanup_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        self.record_completion_cleanup_with_clock(id, generation, || Ok(now))
            .await
    }

    async fn record_completion_cleanup_with_clock(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0 AND EXISTS (SELECT 1 FROM completion_cleanup AS c WHERE c.intent_id = intents.id AND c.claim_generation = ?2 AND c.lease_until > ?3 AND c.runner_absent = 1)",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 0 {
                return Ok(false);
            }
            let retired = connection
                .execute(
                    "UPDATE completion_cleanup SET retry_after = 0, lease_until = 0 WHERE intent_id = ?1 AND claim_generation = ?2",
                    (id, generation),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if retired != 1 {
                return Err(HostError::Journal);
            }
            Ok(true)
        }
        .await;
        finish_transaction(&connection, result).await
    }
}

async fn bind_ids(
    connection: &turso::Connection,
    id: i64,
    runner_container_id: Option<&str>,
    dind_id: Option<&str>,
) -> Result<bool, HostError> {
    let changed = connection
        .execute(
            "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR dind_id IS NULL OR dind_id = ?2)",
            (runner_container_id, dind_id, id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(true);
    }
    same_completion_ids(connection, id, runner_container_id, dind_id).await
}

async fn same_completion_ids(
    connection: &turso::Connection,
    id: i64,
    runner_container_id: Option<&str>,
    dind_id: Option<&str>,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query("SELECT docker_id, dind_id FROM intents WHERE id = ?1", [id])
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let runner: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let dind: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    Ok(
        runner_container_id.is_none_or(|want| runner.as_deref() == Some(want))
            && dind_id.is_none_or(|want| dind.as_deref() == Some(want)),
    )
}
