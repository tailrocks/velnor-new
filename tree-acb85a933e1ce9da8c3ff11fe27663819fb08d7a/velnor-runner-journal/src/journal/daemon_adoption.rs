//! Atomic, auditable adoption of one proven legacy launch on the active Engine.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::reconcile::IntentRow;

use super::{
    Journal, JournalDockerDaemonBinding,
    intent::{INTENT_COLUMNS, intent_row, read_intent_row},
};

/// Result of a NULL-only legacy launch adoption transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyLaunchAdoption {
    /// The exact current Engine binding and adoption receipt were committed.
    Adopted,
    /// A previous attempt committed this same exact adoption.
    AlreadyAdopted,
}

struct AdoptionReceipt<'a> {
    binding: &'a JournalDockerDaemonBinding,
    runner_id: &'a str,
    runner_name: &'a str,
    workflow_run_id: i64,
    scale_set_job_id: &'a str,
    actions_attempt: i64,
    actions_job_id: i64,
    actions_conclusion: Option<&'a str>,
    docker_id: &'a str,
    dind_id: &'a str,
    worker_volume: &'a str,
    outer_network_name: &'a str,
    outer_network_id: &'a str,
    adopted_at_ms: i64,
}

impl Journal {
    /// Load unbound launch rows whose exact Started event was durably observed.
    ///
    /// The result is only a bounded-reconciliation work list. It does not
    /// authorize cleanup, capacity release, or another launch effect.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] if the current schema or rows are invalid.
    pub async fn unbound_started_launches(&self) -> Result<Vec<IntentRow>, HostError> {
        let conn = self.connection().await?;
        let mut query = conn
            .query(
                &format!(
                    "SELECT {INTENT_COLUMNS} FROM intents JOIN linux_launch_started_observations AS started ON started.launch_id = intents.id WHERE intents.kind = 'launch' AND intents.state = 'done' AND intents.effect_state = 'may_have_effect' AND intents.runner_start_state = 'may_have_started' AND intents.cleanup_proven = 0 AND intents.github_runner_id IS NOT NULL AND intents.runner_name IS NOT NULL AND intents.observed_job_id IS NOT NULL AND intents.observed_workflow_run_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM linux_launch_daemon_bindings AS binding WHERE binding.launch_id = intents.id) ORDER BY intents.id"
                ),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut out = Vec::new();
        while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
            out.push(intent_row(&row)?);
        }
        Ok(out)
    }

    /// Adopt one exact legacy generation after authenticated Started, completed
    /// Actions REST, and complete current-Engine inventory have been observed.
    ///
    /// The caller supplies the full row it just read after persisting the REST
    /// receipt. This method re-reads and compares every durable intent field in
    /// one `BEGIN IMMEDIATE` transaction, then inserts the missing Engine
    /// binding and immutable adoption provenance together. It never changes
    /// lifecycle, cleanup, or capacity state. Existing bindings are never
    /// rewritten, and an exact committed retry is idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a stale/incomplete row, missing
    /// Started or completed REST evidence, cleanup already started, an
    /// existing binding, or any failed database operation. A commit error is
    /// ambiguous; retry only the same exact row and binding.
    pub async fn adopt_legacy_launch_on_engine(
        &self,
        expected: &IntentRow,
        binding: &JournalDockerDaemonBinding,
    ) -> Result<LegacyLaunchAdoption, HostError> {
        if self.read_only || expected.id <= 0 {
            return Err(HostError::Journal);
        }
        validate_eligible(expected)?;
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = adopt_in_transaction(&conn, expected, binding).await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }
}

fn validate_eligible(row: &IntentRow) -> Result<(), HostError> {
    let required_text = [
        row.github_runner_id.as_deref(),
        row.runner_name.as_deref(),
        row.observed_job_id.as_deref(),
        row.docker_id.as_deref(),
        row.dind_id.as_deref(),
        row.worker_volume.as_deref(),
        row.outer_network_name.as_deref(),
        row.outer_network_id.as_deref(),
    ];
    let runner_id = row
        .github_runner_id
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    if row.kind != "launch"
        || row.state != super::IntentState::Done
        || row.launch_effect != super::LaunchEffectState::MayHaveEffect
        || row.runner_start_intent != super::RunnerStartIntent::MayHaveStarted
        || !row.remote_terminal
        || row.cleanup_proven
        || row.observed_workflow_run_id.is_none_or(|value| value <= 0)
        || runner_id.is_none_or(|value| value <= 0)
        || row
            .observed_actions_attempt
            .is_none_or(|value| !(1..=8).contains(&value))
        || row.observed_actions_job_id.is_none_or(|value| value <= 0)
        || required_text.iter().any(|value| {
            value.is_none_or(|text| text.is_empty() || text.chars().any(char::is_control))
        })
        || row.runner_name.as_deref().is_some_and(|name| {
            name.len() > 64
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        || row
            .observed_job_id
            .as_deref()
            .is_some_and(|job| job.len() > 256)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn adopt_in_transaction(
    conn: &turso::Connection,
    expected: &IntentRow,
    binding: &JournalDockerDaemonBinding,
) -> Result<LegacyLaunchAdoption, HostError> {
    let current = read_intent_row(conn, expected.id).await?;
    if current != *expected {
        return Err(HostError::Journal);
    }
    validate_eligible(&current)?;
    if started_event_missing(conn, current.id).await? {
        return Err(HostError::Journal);
    }

    if existing_adoption(conn, &current, binding).await?.is_some() {
        return Ok(LegacyLaunchAdoption::AlreadyAdopted);
    }
    if cleanup_already_started(conn, current.id).await? {
        return Err(HostError::Journal);
    }
    if existing_binding(conn, current.id).await? {
        return Err(HostError::Journal);
    }

    let receipt = receipt(&current, binding)?;
    conn.execute(
        "INSERT INTO linux_launch_daemon_bindings (launch_id, endpoint, engine_id) VALUES (?1, ?2, ?3)",
        (current.id, binding.endpoint(), binding.engine_id()),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "INSERT INTO linux_launch_daemon_adoptions (launch_id, endpoint, engine_id, evidence_kind, github_runner_id, runner_name, workflow_run_id, scale_set_job_id, actions_attempt, actions_job_id, actions_conclusion, docker_id, dind_id, worker_volume, outer_network_name, outer_network_id, adopted_at_ms) VALUES (?1, ?2, ?3, 'started_actions_completed_inventory_v1', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        (
            current.id,
            receipt.binding.endpoint(),
            receipt.binding.engine_id(),
            receipt.runner_id,
            receipt.runner_name,
            receipt.workflow_run_id,
            receipt.scale_set_job_id,
            receipt.actions_attempt,
            receipt.actions_job_id,
            receipt.actions_conclusion,
            receipt.docker_id,
            receipt.dind_id,
            receipt.worker_volume,
            receipt.outer_network_name,
            receipt.outer_network_id,
            receipt.adopted_at_ms,
        ),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(LegacyLaunchAdoption::Adopted)
}

fn receipt<'a>(
    row: &'a IntentRow,
    binding: &'a JournalDockerDaemonBinding,
) -> Result<AdoptionReceipt<'a>, HostError> {
    let text = |value: &'a Option<String>| value.as_deref().ok_or(HostError::Journal);
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_millis();
    Ok(AdoptionReceipt {
        binding,
        runner_id: text(&row.github_runner_id)?,
        runner_name: text(&row.runner_name)?,
        workflow_run_id: row.observed_workflow_run_id.ok_or(HostError::Journal)?,
        scale_set_job_id: text(&row.observed_job_id)?,
        actions_attempt: row.observed_actions_attempt.ok_or(HostError::Journal)?,
        actions_job_id: row.observed_actions_job_id.ok_or(HostError::Journal)?,
        actions_conclusion: row.observed_actions_conclusion.as_deref(),
        docker_id: text(&row.docker_id)?,
        dind_id: text(&row.dind_id)?,
        worker_volume: text(&row.worker_volume)?,
        outer_network_name: text(&row.outer_network_name)?,
        outer_network_id: text(&row.outer_network_id)?,
        adopted_at_ms: i64::try_from(observed_at).map_err(|_| HostError::Journal)?,
    })
}

async fn started_event_missing(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT EXISTS (SELECT 1 FROM linux_launch_started_observations WHERE launch_id = ?1) FROM intents WHERE id = ?1 AND kind = 'launch'",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    match value {
        0 => Ok(true),
        1 => Ok(false),
        _ => Err(HostError::Journal),
    }
}

async fn cleanup_already_started(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT EXISTS (SELECT 1 FROM worker_cleanup WHERE launch_id = ?1) OR EXISTS (SELECT 1 FROM worker_cleanup_steps WHERE launch_id = ?1)",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    match rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
    {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}

async fn existing_binding(conn: &turso::Connection, launch_id: i64) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT EXISTS (SELECT 1 FROM linux_launch_daemon_bindings WHERE launch_id = ?1)",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    match rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
    {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}

async fn existing_adoption(
    conn: &turso::Connection,
    row: &IntentRow,
    binding: &JournalDockerDaemonBinding,
) -> Result<Option<()>, HostError> {
    let mut rows = conn
        .query(
            "SELECT endpoint, engine_id, github_runner_id, runner_name, workflow_run_id, scale_set_job_id, actions_attempt, actions_job_id, actions_conclusion, docker_id, dind_id, worker_volume, outer_network_name, outer_network_id FROM linux_launch_daemon_adoptions WHERE launch_id = ?1",
            [row.id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(receipt) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let same = receipt.get::<String>(0).map_err(|_| HostError::Journal)? == binding.endpoint()
        && receipt.get::<String>(1).map_err(|_| HostError::Journal)? == binding.engine_id()
        && receipt.get::<String>(2).map_err(|_| HostError::Journal)?
            == row.github_runner_id.as_deref().ok_or(HostError::Journal)?
        && receipt.get::<String>(3).map_err(|_| HostError::Journal)?
            == row.runner_name.as_deref().ok_or(HostError::Journal)?
        && receipt.get::<i64>(4).map_err(|_| HostError::Journal)?
            == row.observed_workflow_run_id.ok_or(HostError::Journal)?
        && receipt.get::<String>(5).map_err(|_| HostError::Journal)?
            == row.observed_job_id.as_deref().ok_or(HostError::Journal)?
        && receipt.get::<i64>(6).map_err(|_| HostError::Journal)?
            == row.observed_actions_attempt.ok_or(HostError::Journal)?
        && receipt.get::<i64>(7).map_err(|_| HostError::Journal)?
            == row.observed_actions_job_id.ok_or(HostError::Journal)?
        && receipt
            .get::<Option<String>>(8)
            .map_err(|_| HostError::Journal)?
            == row.observed_actions_conclusion
        && receipt.get::<String>(9).map_err(|_| HostError::Journal)?
            == row.docker_id.as_deref().ok_or(HostError::Journal)?
        && receipt.get::<String>(10).map_err(|_| HostError::Journal)?
            == row.dind_id.as_deref().ok_or(HostError::Journal)?
        && receipt.get::<String>(11).map_err(|_| HostError::Journal)?
            == row.worker_volume.as_deref().ok_or(HostError::Journal)?
        && receipt.get::<String>(12).map_err(|_| HostError::Journal)?
            == row
                .outer_network_name
                .as_deref()
                .ok_or(HostError::Journal)?
        && receipt.get::<String>(13).map_err(|_| HostError::Journal)?
            == row.outer_network_id.as_deref().ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || !same {
        return Err(HostError::Journal);
    }
    let mut bindings = conn
        .query(
            "SELECT endpoint, engine_id FROM linux_launch_daemon_bindings WHERE launch_id = ?1",
            [row.id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let exact_binding = bindings
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .is_some_and(|record| {
            record.get::<String>(0).ok().as_deref() == Some(binding.endpoint())
                && record.get::<String>(1).ok().as_deref() == Some(binding.engine_id())
        });
    if !exact_binding
        || bindings
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(Some(()))
}
