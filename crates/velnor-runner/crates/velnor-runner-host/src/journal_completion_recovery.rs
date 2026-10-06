//! Durable leases for incomplete worker-pair cleanup.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::{Journal, token_rejected};

#[path = "journal_completion_recovery_query.rs"]
mod query;

use super::{RecoveryLease, finish_transaction};

const MAX_RECOVERY_BATCH: u32 = 4;
const MAX_RECOVERY_ATTEMPTS: i64 = 5;

impl Journal {
    /// Claim due incomplete launch rows for one scale set, capped at four.
    pub(crate) async fn claim_recovery_batch(
        &self,
        scale_set_id: i64,
        limit: u32,
        lease_seconds: i64,
    ) -> Result<Vec<RecoveryLease>, HostError> {
        self.claim_recovery_batch_now(scale_set_id, limit, lease_seconds, unix_seconds()?)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn claim_recovery_batch_at(
        &self,
        scale_set_id: i64,
        limit: u32,
        lease_seconds: i64,
        now: i64,
    ) -> Result<Vec<RecoveryLease>, HostError> {
        self.claim_recovery_batch_now(scale_set_id, limit, lease_seconds, now)
            .await
    }

    async fn claim_recovery_batch_now(
        &self,
        scale_set_id: i64,
        limit: u32,
        lease_seconds: i64,
        now: i64,
    ) -> Result<Vec<RecoveryLease>, HostError> {
        if scale_set_id <= 0 || lease_seconds <= 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            if now < 0 {
                return Err(HostError::Journal);
            }
            let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Journal)?;
            let ids = query::due_recovery_ids(
                &connection,
                scale_set_id,
                now,
                limit.min(MAX_RECOVERY_BATCH),
            )
            .await?;
            let mut claimed = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(lease) =
                    query::claim_recovery_row(&connection, id, now, lease_until).await?
                {
                    claimed.push(lease);
                }
            }
            Ok(claimed)
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Renew one current claim before a single external request.
    pub(crate) async fn renew_recovery_claim(
        &self,
        lease: &mut RecoveryLease,
        lease_seconds: i64,
    ) -> Result<bool, HostError> {
        self.renew_recovery_claim_now(lease, lease_seconds, unix_seconds()?)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn renew_recovery_claim_at(
        &self,
        lease: &mut RecoveryLease,
        lease_seconds: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        self.renew_recovery_claim_now(lease, lease_seconds, now).await
    }

    async fn renew_recovery_claim_now(
        &self,
        lease: &mut RecoveryLease,
        lease_seconds: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if lease_seconds <= 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            if now < 0 || !query::current_recovery_claim(&connection, lease, now).await? {
                return Ok(false);
            }
            let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE launch_recovery SET lease_until = MAX(lease_until, ?1) WHERE intent_id = ?2 AND generation = ?3 AND lease_until > ?4 AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = ?2)",
                    (lease_until, lease.intent.id, lease.generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Ok(false);
            }
            lease.lease_until = lease.lease_until.max(lease_until);
            Ok(true)
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Bind crash-recovered ids only while this exact generation owns the row.
    pub(crate) async fn bind_recovery_containers(
        &self,
        lease: &mut RecoveryLease,
        runner_id: Option<&str>,
        dind_id: Option<&str>,
    ) -> Result<bool, HostError> {
        self.bind_recovery_containers_now(lease, runner_id, dind_id, unix_seconds()?)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn bind_recovery_containers_at(
        &self,
        lease: &mut RecoveryLease,
        runner_id: Option<&str>,
        dind_id: Option<&str>,
        now: i64,
    ) -> Result<bool, HostError> {
        self.bind_recovery_containers_now(lease, runner_id, dind_id, now)
            .await
    }

    async fn bind_recovery_containers_now(
        &self,
        lease: &mut RecoveryLease,
        runner_id: Option<&str>,
        dind_id: Option<&str>,
        now: i64,
    ) -> Result<bool, HostError> {
        if runner_id.is_some_and(token_rejected) || dind_id.is_some_and(token_rejected) {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            if now < 0 || !query::current_recovery_claim(&connection, lease, now).await? {
                return Ok(false);
            }
            if runner_id.is_some() || dind_id.is_some() {
                let changed = connection
                    .execute(
                        "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR dind_id IS NULL OR dind_id = ?2) AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = ?3)",
                        (runner_id, dind_id, lease.intent.id),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                if changed != 1 {
                    return Ok(false);
                }
                let id = lease.intent.id;
                *lease = query::load_recovery_lease(&connection, id)
                    .await?
                    .ok_or(HostError::Journal)?;
            }
            Ok(true)
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Release an inconclusive claim with a bounded retry delay.
    pub(crate) async fn release_recovery_claim(
        &self,
        lease: &RecoveryLease,
        retry_delay_seconds: i64,
    ) -> Result<bool, HostError> {
        self.release_recovery_claim_now(lease, retry_delay_seconds, unix_seconds()?)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn release_recovery_claim_at(
        &self,
        lease: &RecoveryLease,
        retry_delay_seconds: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        self.release_recovery_claim_now(lease, retry_delay_seconds, now)
            .await
    }

    async fn release_recovery_claim_now(
        &self,
        lease: &RecoveryLease,
        retry_delay_seconds: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if retry_delay_seconds < 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            if now < 0 || !query::current_recovery_claim(&connection, lease, now).await? {
                return Ok(false);
            }
            let retry_after = now.checked_add(retry_delay_seconds).ok_or(HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE launch_recovery SET retry_after = ?1, lease_until = 0 WHERE intent_id = ?2 AND generation = ?3 AND lease_until > ?4 AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = ?2)",
                    (retry_after, lease.intent.id, lease.generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Commit cleanup only for the current lease and an intent without completion work.
    pub(crate) async fn record_recovery_cleanup(
        &self,
        lease: &RecoveryLease,
    ) -> Result<bool, HostError> {
        self.record_recovery_cleanup_now(lease, unix_seconds()?).await
    }

    #[cfg(test)]
    pub(crate) async fn record_recovery_cleanup_at(
        &self,
        lease: &RecoveryLease,
        now: i64,
    ) -> Result<bool, HostError> {
        self.record_recovery_cleanup_now(lease, now).await
    }

    async fn record_recovery_cleanup_now(
        &self,
        lease: &RecoveryLease,
        now: i64,
    ) -> Result<bool, HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            if now < 0 || !query::current_recovery_claim(&connection, lease, now).await? {
                return Ok(false);
            }
            let changed = connection
                .execute(
                    "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = ?1)",
                    [lease.intent.id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Ok(false);
            }
            let retired = connection
                .execute(
                    "UPDATE launch_recovery SET retry_after = 0, lease_until = 0 WHERE intent_id = ?1 AND generation = ?2",
                    (lease.intent.id, lease.generation),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(retired == 1)
        }
        .await;
        finish_transaction(&connection, result).await
    }
}

fn unix_seconds() -> Result<i64, HostError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}
