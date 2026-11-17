// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const CLOCKS: &str = "CREATE TABLE bootstrap_create_claim_clocks (reservation_id INTEGER NOT NULL REFERENCES capacity_reservations(id), resource_kind TEXT NOT NULL CHECK(resource_kind IN ('container','volume','network')), role TEXT NOT NULL, docker_name TEXT NOT NULL, claimed_at INTEGER CHECK(claimed_at>=0), PRIMARY KEY(reservation_id,resource_kind,role))";
const ABSENCES: &str = "CREATE TABLE bootstrap_create_absences (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_unbound_retirement_claims(reservation_id), resource_kind TEXT NOT NULL, role TEXT NOT NULL, docker_name TEXT NOT NULL, observed_at INTEGER NOT NULL, PRIMARY KEY(reservation_id,resource_kind,role), FOREIGN KEY(reservation_id,resource_kind,role) REFERENCES bootstrap_create_claim_clocks(reservation_id,resource_kind,role))";
const RESOURCE: &str = "((clock.resource_kind='container' AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers resource WHERE resource.reservation_id=clock.reservation_id AND resource.role=clock.role AND resource.container_name=clock.docker_name AND resource.create_state!='unclaimed')) OR (clock.resource_kind='volume' AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects resource WHERE resource.reservation_id=clock.reservation_id AND resource.role=clock.role AND resource.docker_name=clock.docker_name AND resource.create_state!='unclaimed')) OR (clock.resource_kind='network' AND clock.role='private' AND EXISTS (SELECT 1 FROM capacity_reservation_networks resource WHERE resource.reservation_id=clock.reservation_id AND resource.network_name=clock.docker_name)))";

fn resources() -> [(&'static str, &'static str, &'static str, &'static str); 3] {
    [
        (
            "container",
            "capacity_reservation_bootstrap_containers",
            "role",
            "container_name",
        ),
        (
            "volume",
            "capacity_reservation_provisioning_objects",
            "role",
            "docker_name",
        ),
        (
            "network",
            "capacity_reservation_networks",
            "'private'",
            "network_name",
        ),
    ]
}

fn objects() -> Vec<(&'static str, String, String)> {
    let mut objects = vec![
        (
            "table",
            "bootstrap_create_claim_clocks".to_owned(),
            CLOCKS.to_owned(),
        ),
        (
            "table",
            "bootstrap_create_absences".to_owned(),
            ABSENCES.to_owned(),
        ),
    ];
    for (kind, table, role, name) in resources() {
        let role = if role == "role" { "NEW.role" } else { role };
        for (suffix, action, condition) in [
            ("insert", "INSERT", "NEW.create_state='requested'"),
            (
                "update",
                "UPDATE OF create_state",
                "NEW.create_state='requested' AND OLD.create_state='unclaimed'",
            ),
        ] {
            let trigger = format!("bootstrap_{kind}_claim_clock_{suffix}");
            objects.push(("trigger", trigger.clone(), format!("CREATE TRIGGER {trigger} AFTER {action} ON {table} WHEN {condition} BEGIN INSERT INTO bootstrap_create_claim_clocks VALUES(NEW.reservation_id,'{kind}',{role},NEW.{name},NULL); END")));
        }
    }
    for table in ["bootstrap_create_claim_clocks", "bootstrap_create_absences"] {
        for action in if table == "bootstrap_create_claim_clocks" {
            vec!["DELETE"]
        } else {
            vec!["UPDATE", "DELETE"]
        } {
            let trigger = format!("{table}_no_{}", action.to_ascii_lowercase());
            objects.push(("trigger", trigger.clone(), format!("CREATE TRIGGER {trigger} BEFORE {action} ON {table} BEGIN SELECT RAISE(ABORT,'Bootstrap claim evidence is immutable.'); END")));
        }
        for (suffix, action) in [
            ("insert", "AFTER INSERT"),
            ("update", "AFTER UPDATE"),
            ("delete", "AFTER DELETE"),
        ] {
            objects.push((
                "trigger",
                format!("{table}_revision_{suffix}"),
                super::super::jit_conflict_settlement::revision_trigger_sql(table, suffix, action),
            ));
        }
    }
    objects.push(("trigger","bootstrap_claim_clock_insert_guard".to_owned(),format!("CREATE TRIGGER bootstrap_claim_clock_insert_guard BEFORE INSERT ON bootstrap_create_claim_clocks WHEN NEW.claimed_at IS NOT NULL OR NOT ({}) BEGIN SELECT RAISE(ABORT,'A bootstrap claim clock requires its committed creation identity.'); END",RESOURCE.replace("clock.","NEW."))));
    objects.push(("trigger","bootstrap_claim_clock_anchor_guard".to_owned(),"CREATE TRIGGER bootstrap_claim_clock_anchor_guard BEFORE UPDATE ON bootstrap_create_claim_clocks WHEN OLD.claimed_at IS NOT NULL OR NEW.claimed_at IS NULL OR NEW.claimed_at<unixepoch()+1 OR NEW.reservation_id!=OLD.reservation_id OR NEW.resource_kind!=OLD.resource_kind OR NEW.role!=OLD.role OR NEW.docker_name!=OLD.docker_name BEGIN SELECT RAISE(ABORT,'The bootstrap claim clock is already anchored.'); END".to_owned()));
    let trigger = "bootstrap_create_absence_insert_guard";
    objects.push(("trigger", trigger.to_owned(), format!("CREATE TRIGGER {trigger} BEFORE INSERT ON bootstrap_create_absences WHEN NOT EXISTS (SELECT 1 FROM bootstrap_create_claim_clocks clock JOIN capacity_reservation_unbound_retirement_claims claim ON claim.reservation_id=clock.reservation_id JOIN capacity_reservations reservation ON reservation.id=claim.reservation_id WHERE clock.reservation_id=NEW.reservation_id AND clock.resource_kind=NEW.resource_kind AND clock.role=NEW.role AND clock.docker_name=NEW.docker_name AND NEW.observed_at>=clock.claimed_at+60 AND claim.lease_until>unixepoch() AND reservation.state='uncertain') BEGIN SELECT RAISE(ABORT,'The bootstrap claim deadline has not elapsed.'); END")));
    objects
}

pub(super) async fn install(conn: &turso::Connection) -> Result<(), HostError> {
    for (_, _, sql) in objects() {
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (kind, table, role, name) in resources() {
        let sql = format!(
            "INSERT INTO bootstrap_create_claim_clocks SELECT reservation_id,'{kind}',{role},{name},NULL FROM {table} WHERE create_state IN ('requested','uncertain')"
        );
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    super::creation_targets::install_absence_guards(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_objects(conn, objects()).await?;
    let mut clocks = conn
        .query(
            &format!(
                "SELECT 1 FROM bootstrap_create_claim_clocks clock WHERE NOT ({RESOURCE}) LIMIT 1"
            ),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if clocks
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .is_some()
    {
        return Err(HostError::Journal);
    }
    let mut rows = conn.query("SELECT 1 FROM bootstrap_create_absences absence LEFT JOIN bootstrap_create_claim_clocks clock ON clock.reservation_id=absence.reservation_id AND clock.resource_kind=absence.resource_kind AND clock.role=absence.role WHERE clock.reservation_id IS NULL OR clock.docker_name!=absence.docker_name OR absence.observed_at<clock.claimed_at+60 LIMIT 1", ()).await.map_err(|_|HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) fn absent(kind: &str, role: &str, name: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM bootstrap_create_absences absence WHERE absence.reservation_id=resource.reservation_id AND absence.resource_kind='{kind}' AND absence.role={role} AND absence.docker_name=resource.{name})"
    )
}

pub(super) fn with_absence(predicate: &str) -> String {
    let (kind, role, name) = if predicate.contains("capacity_reservation_bootstrap_containers") {
        ("container", "resource.role", "container_name")
    } else if predicate.contains("capacity_reservation_provisioning_objects") {
        ("volume", "resource.role", "docker_name")
    } else {
        ("network", "'private'", "network_name")
    };
    let end = predicate.len() - 1;
    format!(
        "{} AND NOT {})",
        &predicate[..end],
        absent(kind, role, name)
    )
}
