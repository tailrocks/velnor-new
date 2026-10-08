//! Enforce the durable cleanup side-effect order before each external action.

use crate::error::HostError;

use super::read::{children_drained, cleanup_children, cleanup_row, step_completed, step_intended};
use super::validation::container_id;

pub(super) async fn validate_before_step(
    conn: &turso::Connection,
    launch_id: i64,
    step: &str,
) -> Result<(), HostError> {
    let cleanup = cleanup_row(conn, launch_id)
        .await?
        .ok_or(HostError::Journal)?;
    match step {
        "runner-termination" => {
            if cleanup.runner_start_observation.is_none() {
                return Err(HostError::Journal);
            }
        }
        "diagnostics-retention" => {
            require_completed(conn, launch_id, "runner-termination").await?;
        }
        "child-enumeration" => {
            require_completed(conn, launch_id, "diagnostics-retention").await?;
        }
        "children-drained" => {
            require_completed(conn, launch_id, "child-enumeration").await?;
            require_completed(conn, launch_id, "diagnostics-retention").await?;
            validate_child_removals(conn, launch_id).await?;
        }
        "dind-termination" => {
            require_completed(conn, launch_id, "runner-termination").await?;
            require_completed(conn, launch_id, "diagnostics-retention").await?;
            require_drained(conn, launch_id).await?;
        }
        "runner-removal" => {
            require_completed(conn, launch_id, "dind-termination").await?;
            require_completed(conn, launch_id, "diagnostics-retention").await?;
            require_drained(conn, launch_id).await?;
        }
        "dind-removal" => {
            require_completed(conn, launch_id, "dind-termination").await?;
            require_completed(conn, launch_id, "runner-removal").await?;
        }
        "outer-network-removal" => {
            require_completed(conn, launch_id, "dind-removal").await?;
            if cleanup.outer_network_name.is_none() || cleanup.outer_network_id.is_none() {
                return Err(HostError::Journal);
            }
        }
        "volume-removal" => {
            require_completed(conn, launch_id, "dind-removal").await?;
            if cleanup.outer_network_name.is_some() {
                require_completed(conn, launch_id, "outer-network-removal").await?;
            }
        }
        _ if child_removal(step).is_some() => {
            require_completed(conn, launch_id, "child-enumeration").await?;
            if children_drained(conn, launch_id).await? {
                return Err(HostError::Journal);
            }
            let (kind, id) = child_removal(step).ok_or(HostError::Journal)?;
            let children = cleanup_children(conn, launch_id).await?;
            let observed = match kind {
                "container" => children.containers.iter().any(|value| value == id),
                "network" => children.networks.iter().any(|value| value == id),
                _ => false,
            };
            if !observed {
                return Err(HostError::Journal);
            }
        }
        _ => return Err(HostError::Journal),
    }
    Ok(())
}

pub(super) async fn validate_after_step(
    conn: &turso::Connection,
    launch_id: i64,
    step: &str,
) -> Result<(), HostError> {
    validate_before_step(conn, launch_id, step).await?;
    if !step_intended(conn, launch_id, step).await? {
        return Err(HostError::Journal);
    }
    match step {
        "diagnostics-retention" => {
            let cleanup = cleanup_row(conn, launch_id)
                .await?
                .ok_or(HostError::Journal)?;
            if !cleanup.diagnostics_recorded {
                return Err(HostError::Journal);
            }
        }
        "children-drained" => {
            if !children_drained(conn, launch_id).await? {
                return Err(HostError::Journal);
            }
            validate_child_removals(conn, launch_id).await?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) async fn validate_child_removals(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM worker_cleanup_resources r LEFT JOIN worker_cleanup_steps s ON s.launch_id = r.launch_id AND s.step_key = 'child-' || r.resource_kind || ':' || r.resource_id AND s.completed = 1 WHERE r.launch_id = ?1 AND s.launch_id IS NULL",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let remaining = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if remaining == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn require_drained(conn: &turso::Connection, launch_id: i64) -> Result<(), HostError> {
    require_completed(conn, launch_id, "children-drained").await?;
    if !children_drained(conn, launch_id).await? {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn require_completed(
    conn: &turso::Connection,
    launch_id: i64,
    step: &str,
) -> Result<(), HostError> {
    if step_completed(conn, launch_id, step).await? {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn child_removal(step: &str) -> Option<(&'static str, &str)> {
    let (kind, id) = step
        .strip_prefix("child-container:")
        .map(|id| ("container", id))
        .or_else(|| {
            step.strip_prefix("child-network:")
                .map(|id| ("network", id))
        })?;
    container_id(id).then_some((kind, id))
}
