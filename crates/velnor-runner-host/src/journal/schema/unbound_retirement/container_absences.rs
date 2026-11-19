// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const TABLE: &str = "CREATE TABLE unbound_container_absences (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_unbound_retirement_claims(reservation_id), role TEXT NOT NULL, docker_name TEXT NOT NULL, claim_generation INTEGER NOT NULL CHECK (claim_generation > 0), PRIMARY KEY (reservation_id, role))";

const INSERT_GUARD: &str = "CREATE TRIGGER unbound_container_absence_insert_guard BEFORE INSERT ON unbound_container_absences WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_claims AS claim JOIN capacity_reservation_bootstrap_containers AS container ON container.reservation_id = claim.reservation_id WHERE claim.reservation_id = NEW.reservation_id AND claim.claim_generation = NEW.claim_generation AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND container.role = NEW.role AND container.container_name = NEW.docker_name) BEGIN SELECT RAISE(ABORT, 'A container absence requires its live retirement claim.'); END";

const PROGRESS_GUARD: &str = "CREATE TRIGGER unbound_retirement_container_absence_guard BEFORE INSERT ON capacity_reservation_unbound_retirement_progress WHEN NEW.phase = 'containers_absent' AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS container WHERE container.reservation_id = NEW.reservation_id AND (container.container_id IS NOT NULL OR container.create_state != 'unclaimed' OR container.start_state != 'not_requested') AND NOT EXISTS (SELECT 1 FROM unbound_container_absences AS absence WHERE absence.reservation_id = container.reservation_id AND absence.role = container.role AND absence.docker_name = container.container_name)) BEGIN SELECT RAISE(ABORT, 'A bootstrap container is not proven absent.'); END";

const IMMUTABLE: [(&str, &str); 2] = [("update", "UPDATE"), ("delete", "DELETE")];

fn objects() -> Vec<(&'static str, String, String)> {
    let mut objects = vec![
        (
            "table",
            "unbound_container_absences".to_owned(),
            TABLE.to_owned(),
        ),
        (
            "trigger",
            "unbound_container_absence_insert_guard".to_owned(),
            INSERT_GUARD.to_owned(),
        ),
        (
            "trigger",
            "unbound_retirement_container_absence_guard".to_owned(),
            PROGRESS_GUARD.to_owned(),
        ),
    ];
    for (suffix, action) in IMMUTABLE {
        let name = format!("unbound_container_absences_no_{suffix}");
        let sql = format!(
            "CREATE TRIGGER {name} BEFORE {action} ON unbound_container_absences BEGIN SELECT RAISE(ABORT, 'Container absence evidence is immutable.'); END"
        );
        objects.push(("trigger", name, sql));
    }
    for (suffix, action) in [
        ("insert", "AFTER INSERT"),
        ("update", "AFTER UPDATE"),
        ("delete", "AFTER DELETE"),
    ] {
        objects.push((
            "trigger",
            format!("unbound_container_absences_revision_{suffix}"),
            super::super::jit_conflict_settlement::revision_trigger_sql(
                "unbound_container_absences",
                suffix,
                action,
            ),
        ));
    }
    objects
}

pub(super) async fn migrate(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate(conn).await?;
    let objects = objects();
    for (kind, name, _) in objects.iter().rev() {
        conn.execute(
            &format!("DROP {} IF EXISTS {name}", kind.to_uppercase()),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    for (_, _, sql) in objects {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute("PRAGMA user_version = 56", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_objects(conn, objects()).await
}
