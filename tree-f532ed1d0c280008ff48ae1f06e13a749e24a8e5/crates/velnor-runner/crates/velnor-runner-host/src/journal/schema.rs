//! Ordered local journal schema migrations.

use crate::error::HostError;

pub(super) async fn ensure_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let mut has_dind = false;
    let mut has_volume = false;
    let mut missing = ["scale_set_id", "request_id", "runner_name", "docker_engine_id", "launch_phase"];
    let mut rows = conn
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        has_dind |= name == "dind_id";
        has_volume |= name == "worker_volume";
        missing.retain(|column| *column != name.as_str());
    }
    if !has_dind {
        conn.execute("ALTER TABLE intents ADD COLUMN dind_id TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if !has_volume {
        conn.execute("ALTER TABLE intents ADD COLUMN worker_volume TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for column in missing {
        let declaration = match column {
            "scale_set_id" | "request_id" => "INTEGER",
            "runner_name" | "docker_engine_id" | "launch_phase" => "TEXT",
            _ => return Err(HostError::Journal),
        };
        conn.execute(
            &format!("ALTER TABLE intents ADD COLUMN {column} {declaration}"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
