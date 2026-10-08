//! Journal methods for durable worker cleanup checkpoints.

use crate::error::HostError;
use std::ops::AsyncFnOnce;

use super::order::{validate_after_step, validate_before_step};
use super::proof::{validate_completed, validate_ready};
use super::read::{
    children_drained, cleanup_children, cleanup_row, diagnostics_match, ensure_cleanup_open,
    step_completed, step_intended, validate_generation,
};
use super::validation::{
    policy_fields, post_action_fields, validate_begin, validate_children, validate_diagnostics,
    validate_step,
};
use super::{
    CleanupCheckpointIdentity, CleanupChildren, CleanupDiagnostics, CleanupStopPolicy,
    RunnerStartObservation,
};
use crate::journal::Journal;
use crate::journal::PostActionDisposition;

impl Journal {
    /// Fence cleanup to an exact, already journaled worker generation.
    ///
    /// Acquisition-required rows must have a confirmed `AcquireJobs` result;
    /// cleanup cannot resolve an ambiguous request. Repeated calls must use
    /// the same identity and cleanup policy.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an unknown effect, mismatched row,
    /// invalid identity, or failed transaction.
    pub async fn begin_cleanup(
        &self,
        identity: &CleanupCheckpointIdentity,
        post_actions: &PostActionDisposition,
        policy: &CleanupStopPolicy,
    ) -> Result<(), HostError> {
        validate_begin(identity, post_actions, policy)?;
        self.with_immediate(async |conn| {
            validate_generation(conn, identity, post_actions).await?;
            let (policy_name, grace, reason) = policy_fields(policy);
            let (post_state, post_reason) = post_action_fields(post_actions);
            let existing = cleanup_row(conn, identity.launch_id).await?;
            if let Some(existing) = existing {
                return if existing.policy == policy_name
                    && existing.grace == grace
                    && existing.policy_reason == reason
                    && existing.post_state == post_state
                    && existing.post_reason == post_reason
                    && existing.outer_network_name == identity.outer_network_name
                    && existing.outer_network_id == identity.outer_network_id
                    && !existing.complete
                {
                    Ok(())
                } else {
                    Err(HostError::Journal)
                };
            }
            conn.execute(
                "INSERT INTO worker_cleanup (launch_id, post_action_disposition, post_action_reason_class, stop_policy, stop_grace_seconds, stop_reason_class, outer_network_name, outer_network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (
                    identity.launch_id,
                    post_state,
                    post_reason,
                    policy_name,
                    grace,
                    reason,
                    identity.outer_network_name.clone(),
                    identity.outer_network_id.clone(),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Persist an external cleanup step intent before performing its effect.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an invalid step or missing cleanup lease.
    pub async fn cleanup_before(&self, launch_id: i64, step_key: &str) -> Result<(), HostError> {
        validate_step(step_key)?;
        self.with_immediate(async |conn| {
            ensure_cleanup_open(conn, launch_id).await?;
            validate_before_step(conn, launch_id, step_key).await?;
            conn.execute(
                "INSERT OR IGNORE INTO worker_cleanup_steps (launch_id, step_key, completed) VALUES (?1, ?2, 0)",
                (launch_id, step_key),
            )
            .await
            .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Persist that a previously journaled external cleanup step completed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the step has no prior intent.
    pub async fn cleanup_after(&self, launch_id: i64, step_key: &str) -> Result<(), HostError> {
        validate_step(step_key)?;
        self.with_immediate(async |conn| {
            ensure_cleanup_open(conn, launch_id).await?;
            validate_after_step(conn, launch_id, step_key).await?;
            let changed = conn
                .execute(
                    "UPDATE worker_cleanup_steps SET completed = 1 WHERE launch_id = ?1 AND step_key = ?2 AND completed = 0",
                    (launch_id, step_key),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || step_completed(conn, launch_id, step_key).await? {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Persist exact child IDs observed before cleanup requests are sent.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for invalid IDs or changed inventory after
    /// a final empty observation was already committed.
    pub async fn observe_cleanup_children(
        &self,
        launch_id: i64,
        children: &CleanupChildren,
    ) -> Result<(), HostError> {
        validate_children(children)?;
        self.with_immediate(async |conn| {
            ensure_cleanup_open(conn, launch_id).await?;
            validate_before_step(conn, launch_id, "child-enumeration").await?;
            if !step_intended(conn, launch_id, "child-enumeration").await?
                || step_completed(conn, launch_id, "children-drained").await?
            {
                return Err(HostError::Journal);
            }
            let existing = cleanup_children(conn, launch_id).await?;
            let drained = children_drained(conn, launch_id).await?;
            if drained {
                return if existing == *children {
                    Ok(())
                } else {
                    Err(HostError::Journal)
                };
            }
            for (kind, ids) in [("container", &children.containers), ("network", &children.networks)] {
                for id in ids {
                    conn.execute(
                        "INSERT OR IGNORE INTO worker_cleanup_resources (launch_id, resource_kind, resource_id) VALUES (?1, ?2, ?3)",
                        (launch_id, kind, id.clone()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                }
            }
            Ok(())
        })
        .await
    }

    /// Return all child IDs ever observed for this generation, in stable order.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the cleanup generation is missing or
    /// its inventory is corrupt.
    pub async fn cleanup_children(&self, launch_id: i64) -> Result<CleanupChildren, HostError> {
        let conn = self.connection().await?;
        ensure_cleanup_open(&conn, launch_id).await?;
        cleanup_children(&conn, launch_id).await
    }

    /// Return child evidence only after an empty final private-daemon query was recorded.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the cleanup state is invalid.
    pub async fn prior_cleanup_children_drained(
        &self,
        launch_id: i64,
    ) -> Result<Option<CleanupChildren>, HostError> {
        let conn = self.connection().await?;
        ensure_cleanup_open(&conn, launch_id).await?;
        if children_drained(&conn, launch_id).await? {
            Ok(Some(cleanup_children(&conn, launch_id).await?))
        } else {
            Ok(None)
        }
    }

    /// Record that a second private-daemon query returned empty.
    ///
    /// The supplied inventory must equal every resource observed by this cleanup
    /// generation, proving that all identified children were accounted for.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the observed inventory does not match.
    pub async fn record_cleanup_children_drained(
        &self,
        launch_id: i64,
        children: &CleanupChildren,
    ) -> Result<(), HostError> {
        validate_children(children)?;
        self.with_immediate(async |conn| {
            ensure_cleanup_open(conn, launch_id).await?;
            validate_before_step(conn, launch_id, "children-drained").await?;
            if !step_intended(conn, launch_id, "children-drained").await?
                || cleanup_children(conn, launch_id).await? != *children
            {
                return Err(HostError::Journal);
            }
            conn.execute(
                "UPDATE worker_cleanup SET children_drained = 1 WHERE launch_id = ?1 AND children_drained IN (0, 1)",
                [launch_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Persist the redacted diagnostic receipt before outer resources are removed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an invalid or conflicting receipt.
    pub async fn record_cleanup_diagnostics(
        &self,
        launch_id: i64,
        receipt: &CleanupDiagnostics,
    ) -> Result<(), HostError> {
        validate_diagnostics(receipt)?;
        self.with_immediate(async |conn| {
            ensure_cleanup_open(conn, launch_id).await?;
            validate_before_step(conn, launch_id, "diagnostics-retention").await?;
            let cleanup = cleanup_row(conn, launch_id).await?.ok_or(HostError::Journal)?;
            let start_matches = cleanup.runner_start_observation
                == Some(if receipt.source_absent {
                    RunnerStartObservation::NeverStarted
                } else {
                    RunnerStartObservation::MayHaveStarted
                });
            if (cleanup.post_state == "not_run") != receipt.source_absent
                || !start_matches
                || !step_intended(conn, launch_id, "diagnostics-retention").await?
            {
                return Err(HostError::Journal);
            }
            if cleanup.diagnostics_recorded {
                return if diagnostics_match(conn, launch_id, receipt).await? {
                    Ok(())
                } else {
                    Err(HostError::Journal)
                };
            }
            if step_completed(conn, launch_id, "runner-removal").await?
                || step_completed(conn, launch_id, "dind-removal").await?
            {
                return Err(HostError::Journal);
            }
            let changed = conn
                .execute(
                    "UPDATE worker_cleanup SET diagnostics_recorded = 1, diagnostics_relative_path = ?1, diagnostics_sha256 = ?2, diagnostics_bytes = ?3, diagnostics_redacted = ?4, diagnostics_retained = ?5, diagnostics_source_absent = ?6 WHERE launch_id = ?7 AND diagnostics_recorded = 0",
                    (
                        receipt.relative_path.clone(),
                        receipt.sha256.clone(),
                        i64::try_from(receipt.bytes).map_err(|_| HostError::Journal)?,
                        i64::from(receipt.redacted),
                        i64::from(receipt.retained),
                        i64::from(receipt.source_absent),
                        launch_id,
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || diagnostics_match(conn, launch_id, receipt).await? {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }
}

impl Journal {
    pub(super) async fn with_immediate<T, F>(&self, operation: F) -> Result<T, HostError>
    where
        F: AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
    {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = operation(&conn).await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }
}

impl Journal {
    pub(in crate::journal::cleanup) async fn validate_cleanup_ready(
        conn: &turso::Connection,
        proof: &super::super::CleanupRecord,
    ) -> Result<(), HostError> {
        validate_ready(conn, proof).await
    }

    pub(in crate::journal::cleanup) async fn validate_cleanup_complete(
        conn: &turso::Connection,
        proof: &super::super::CleanupRecord,
    ) -> Result<(), HostError> {
        validate_completed(conn, proof).await
    }

    pub(in crate::journal::cleanup) async fn mark_cleanup_complete(
        conn: &turso::Connection,
        launch_id: i64,
    ) -> Result<(), HostError> {
        let changed = conn
            .execute(
                "UPDATE worker_cleanup SET complete = 1 WHERE launch_id = ?1 AND complete = 0 AND children_drained = 1 AND diagnostics_recorded = 1",
                [launch_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            let intent_changed = conn
                .execute(
                    "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0 AND remote_terminal = 1 AND effect_state = 'may_have_effect'",
                    [launch_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if intent_changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        } else {
            Err(HostError::Journal)
        }
    }
}
