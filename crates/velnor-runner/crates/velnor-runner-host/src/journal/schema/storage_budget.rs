//! Immutable storage profiles and per-reservation storage evidence.

use std::collections::HashMap;

use crate::error::HostError;

use super::{ColumnInfo, read_table_columns};

const TABLES: [&str; 8] = [
    "host_storage_profiles",
    "host_storage_profile_pools",
    "host_storage_profile_roles",
    "host_storage_profile_current",
    "capacity_reservation_storage",
    "capacity_reservation_storage_roles",
    "capacity_reservation_storage_samples",
    "capacity_reservation_storage_pools",
];

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for statement in statements() {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for statement in triggers() {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    let expected = expected_columns();
    let constraints = required_sql();
    for (index, table) in TABLES.iter().enumerate() {
        validate_table(conn, table, &expected[index], &constraints[index]).await?;
    }
    validate_triggers(conn).await?;
    crate::journal::connection::validate_storage_budget_schema(conn).await
}

async fn validate_triggers(conn: &turso::Connection) -> Result<(), HostError> {
    let expected = trigger_contracts();
    let mut rows = conn
        .query(
            "SELECT name, tbl_name, sql FROM sqlite_schema WHERE type = 'trigger'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut found = 0;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(0).map_err(|_| HostError::Journal)?;
        let table: String = row.get(1).map_err(|_| HostError::Journal)?;
        let Some((_, expected_table, before, action)) =
            expected.iter().find(|contract| contract.0 == name)
        else {
            if TABLES.contains(&table.as_str()) {
                return Err(HostError::Journal);
            }
            continue;
        };
        let sql = row
            .get::<String>(2)
            .map_err(|_| HostError::Journal)?
            .to_ascii_lowercase()
            .replace(char::is_whitespace, "");
        if table != *expected_table
            || !sql.contains(&before.to_ascii_lowercase().replace(char::is_whitespace, ""))
            || !sql.contains(&action.to_ascii_lowercase().replace(char::is_whitespace, ""))
        {
            return Err(HostError::Journal);
        }
        found += 1;
    }
    if found == expected.len() {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn trigger_contracts() -> [(&'static str, &'static str, &'static str, &'static str); 16] {
    [
        (
            "host_storage_profiles_no_update",
            "host_storage_profiles",
            "before update on host_storage_profiles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profiles_no_delete",
            "host_storage_profiles",
            "before delete on host_storage_profiles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_pools_no_update",
            "host_storage_profile_pools",
            "before update on host_storage_profile_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_pools_no_delete",
            "host_storage_profile_pools",
            "before delete on host_storage_profile_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_roles_no_update",
            "host_storage_profile_roles",
            "before update on host_storage_profile_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_roles_no_delete",
            "host_storage_profile_roles",
            "before delete on host_storage_profile_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_roles_alias_target",
            "host_storage_profile_roles",
            "before insert on host_storage_profile_roles",
            "raise(abort,'invalid storage role alias')",
        ),
        (
            "capacity_reservation_storage_no_update",
            "capacity_reservation_storage",
            "before update on capacity_reservation_storage",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_no_delete",
            "capacity_reservation_storage",
            "before delete on capacity_reservation_storage",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_roles_no_update",
            "capacity_reservation_storage_roles",
            "before update on capacity_reservation_storage_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_roles_no_delete",
            "capacity_reservation_storage_roles",
            "before delete on capacity_reservation_storage_roles",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_samples_no_update",
            "capacity_reservation_storage_samples",
            "before update on capacity_reservation_storage_samples",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_samples_no_delete",
            "capacity_reservation_storage_samples",
            "before delete on capacity_reservation_storage_samples",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_pools_no_update",
            "capacity_reservation_storage_pools",
            "before update on capacity_reservation_storage_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "capacity_reservation_storage_pools_no_delete",
            "capacity_reservation_storage_pools",
            "before delete on capacity_reservation_storage_pools",
            "raise(abort,'immutable storage evidence')",
        ),
        (
            "host_storage_profile_current_no_delete",
            "host_storage_profile_current",
            "before delete on host_storage_profile_current",
            "raise(abort,'immutable storage evidence')",
        ),
    ]
}

fn triggers() -> [&'static str; 16] {
    [
        "CREATE TRIGGER IF NOT EXISTS host_storage_profiles_no_update BEFORE UPDATE ON host_storage_profiles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profiles_no_delete BEFORE DELETE ON host_storage_profiles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_pools_no_update BEFORE UPDATE ON host_storage_profile_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_pools_no_delete BEFORE DELETE ON host_storage_profile_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_roles_no_update BEFORE UPDATE ON host_storage_profile_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_roles_no_delete BEFORE DELETE ON host_storage_profile_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_roles_alias_target BEFORE INSERT ON host_storage_profile_roles WHEN NEW.alias_of IS NOT NULL AND (NEW.role NOT IN ('cache-live', 'cache-stage') OR NEW.alias_of != 'runner-work' OR NOT EXISTS (SELECT 1 FROM host_storage_profile_roles target WHERE target.generation_id = NEW.generation_id AND target.role = NEW.alias_of AND target.alias_of IS NULL AND target.pool_id = NEW.pool_id AND target.medium = NEW.medium AND NEW.max_bytes <= target.max_bytes AND NEW.max_inodes <= target.max_inodes)) BEGIN SELECT RAISE(ABORT, 'invalid storage role alias'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_no_update BEFORE UPDATE ON capacity_reservation_storage BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_no_delete BEFORE DELETE ON capacity_reservation_storage BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_roles_no_update BEFORE UPDATE ON capacity_reservation_storage_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_roles_no_delete BEFORE DELETE ON capacity_reservation_storage_roles BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_samples_no_update BEFORE UPDATE ON capacity_reservation_storage_samples BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_samples_no_delete BEFORE DELETE ON capacity_reservation_storage_samples BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_pools_no_update BEFORE UPDATE ON capacity_reservation_storage_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_pools_no_delete BEFORE DELETE ON capacity_reservation_storage_pools BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
        "CREATE TRIGGER IF NOT EXISTS host_storage_profile_current_no_delete BEFORE DELETE ON host_storage_profile_current BEGIN SELECT RAISE(ABORT, 'immutable storage evidence'); END",
    ]
}

async fn validate_table(
    conn: &turso::Connection,
    table: &str,
    expected: &[Shape],
    fragments: &[&str],
) -> Result<(), HostError> {
    let columns = read_table_columns(conn, table).await?;
    if columns.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for shape in expected {
        if !column_matches(&columns, shape) {
            return Err(HostError::Journal);
        }
    }
    validate_sql(conn, table, fragments).await
}

fn column_matches(columns: &HashMap<String, ColumnInfo>, shape: &Shape) -> bool {
    columns.get(shape.name).is_some_and(|column| {
        column.declared_type.trim().eq_ignore_ascii_case(shape.kind)
            && column.not_null == shape.not_null
            && column.default_value.as_deref() == shape.default
            && column.primary_key_position == shape.primary_key
    })
}

async fn validate_sql(
    conn: &turso::Connection,
    table: &str,
    fragments: &[&str],
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?
        .to_ascii_lowercase()
        .replace(char::is_whitespace, "");
    if fragments.iter().any(|fragment| {
        !sql.contains(
            &fragment
                .to_ascii_lowercase()
                .replace(char::is_whitespace, ""),
        )
    }) {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn statements() -> [&'static str; 10] {
    [
        "CREATE TABLE IF NOT EXISTS host_storage_profiles (generation_id TEXT NOT NULL PRIMARY KEY CHECK (length(generation_id) = 32 AND generation_id NOT GLOB '*[^0-9a-f]*'), profile_digest TEXT NOT NULL CHECK (length(profile_digest) = 64 AND profile_digest NOT GLOB '*[^0-9a-f]*'))",
        "CREATE TABLE IF NOT EXISTS host_storage_profile_pools (generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs', 'unknown')), max_bytes INTEGER NOT NULL CHECK (max_bytes > 0), max_inodes INTEGER NOT NULL CHECK (max_inodes >= 0), reserve_bytes INTEGER NOT NULL CHECK (reserve_bytes >= 0 AND reserve_bytes <= max_bytes), reserve_inodes INTEGER NOT NULL CHECK (reserve_inodes >= 0 AND reserve_inodes <= max_inodes), CHECK ((inode_policy = 'finite' AND max_inodes > 0) OR (inode_policy IN ('dynamic-btrfs', 'unknown') AND medium = 'disk' AND max_inodes = 0 AND reserve_inodes = 0)), PRIMARY KEY (generation_id, pool_id))",
        "CREATE TABLE IF NOT EXISTS host_storage_profile_roles (generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'cache-live', 'cache-stage', 'engine-metadata')), pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), max_bytes INTEGER NOT NULL CHECK (max_bytes >= 0), max_inodes INTEGER NOT NULL CHECK (max_inodes >= 0), retained_memory_bytes INTEGER NOT NULL CHECK (retained_memory_bytes >= 0), alias_of TEXT CHECK (alias_of IS NULL OR (alias_of IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'cache-live', 'cache-stage', 'engine-metadata') AND alias_of != role)), CHECK ((alias_of IS NULL AND role NOT IN ('cache-live', 'cache-stage') AND max_bytes > 0 AND max_inodes > 0 AND ((medium = 'disk' AND retained_memory_bytes = 0) OR (medium = 'tmpfs' AND retained_memory_bytes = max_bytes))) OR (alias_of = 'runner-work' AND role IN ('cache-live', 'cache-stage') AND max_bytes > 0 AND max_inodes > 0 AND retained_memory_bytes = 0)), PRIMARY KEY (generation_id, role), FOREIGN KEY (generation_id, pool_id) REFERENCES host_storage_profile_pools(generation_id, pool_id), FOREIGN KEY (generation_id, alias_of) REFERENCES host_storage_profile_roles(generation_id, role))",
        "CREATE TABLE IF NOT EXISTS host_storage_profile_current (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id))",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_storage (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservations(id), profile_generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), profile_digest TEXT NOT NULL CHECK (length(profile_digest) = 64 AND profile_digest NOT GLOB '*[^0-9a-f]*'), instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'), retained_memory_bytes INTEGER NOT NULL CHECK (retained_memory_bytes >= 0))",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_storage_roles (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_storage(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'cache-live', 'cache-stage', 'engine-metadata')), pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs')), filesystem_kind TEXT NOT NULL CHECK (filesystem_kind IN ('btrfs', 'tmpfs', 'other')), charge_bytes INTEGER NOT NULL CHECK (charge_bytes > 0), charge_inodes INTEGER NOT NULL CHECK (charge_inodes > 0), retained_memory_bytes INTEGER NOT NULL CHECK (retained_memory_bytes >= 0), CHECK ((medium = 'disk' AND filesystem_kind IN ('btrfs', 'other') AND retained_memory_bytes = 0) OR (medium = 'tmpfs' AND filesystem_kind = 'tmpfs' AND retained_memory_bytes = charge_bytes)), CHECK (inode_policy != 'dynamic-btrfs' OR (medium = 'disk' AND filesystem_kind = 'btrfs')), PRIMARY KEY (reservation_id, role))",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_storage_samples (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_storage(reservation_id), sample_generation_id TEXT NOT NULL CHECK (length(sample_generation_id) = 32 AND sample_generation_id NOT GLOB '*[^0-9a-f]*'), profile_generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), profile_digest TEXT NOT NULL CHECK (length(profile_digest) = 64 AND profile_digest NOT GLOB '*[^0-9a-f]*'), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms > 0), guest_available_bytes INTEGER, ancestor_memory_max_bytes INTEGER, ancestor_memory_current_bytes INTEGER, ancestor_swap_max_bytes INTEGER, ancestor_swap_current_bytes INTEGER, CHECK ((guest_available_bytes IS NULL AND ancestor_memory_max_bytes IS NULL AND ancestor_memory_current_bytes IS NULL AND ancestor_swap_max_bytes IS NULL AND ancestor_swap_current_bytes IS NULL) OR (guest_available_bytes > 0 AND ancestor_memory_max_bytes > 0 AND ancestor_memory_current_bytes >= 0 AND ancestor_memory_current_bytes <= ancestor_memory_max_bytes AND ancestor_swap_max_bytes >= 0 AND ancestor_swap_current_bytes >= 0 AND ancestor_swap_current_bytes <= ancestor_swap_max_bytes)), PRIMARY KEY (reservation_id, sample_generation_id))",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_storage_pools (reservation_id INTEGER NOT NULL, sample_generation_id TEXT NOT NULL, pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), filesystem_kind TEXT NOT NULL CHECK (filesystem_kind IN ('btrfs', 'tmpfs', 'other')), inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs')), total_bytes INTEGER NOT NULL CHECK (total_bytes > 0), available_bytes INTEGER NOT NULL CHECK (available_bytes >= 0 AND available_bytes <= total_bytes), total_inodes INTEGER NOT NULL CHECK (total_inodes >= 0), available_inodes INTEGER NOT NULL CHECK (available_inodes >= 0 AND available_inodes <= total_inodes), CHECK ((inode_policy = 'finite' AND total_inodes > 0) OR (inode_policy = 'dynamic-btrfs' AND medium = 'disk' AND filesystem_kind = 'btrfs' AND total_inodes = 0 AND available_inodes = 0)), PRIMARY KEY (reservation_id, sample_generation_id, pool_id), FOREIGN KEY (reservation_id, sample_generation_id) REFERENCES capacity_reservation_storage_samples(reservation_id, sample_generation_id))",
        "CREATE INDEX IF NOT EXISTS capacity_storage_roles_pool ON capacity_reservation_storage_roles(pool_id, filesystem_instance_digest)",
        "CREATE INDEX IF NOT EXISTS capacity_storage_samples_profile ON capacity_reservation_storage_samples(profile_generation_id, sample_generation_id)",
    ]
}

fn expected_columns() -> [Vec<Shape>; 8] {
    [
        vec![
            Shape::new("generation_id", "TEXT", true, None, 1),
            Shape::new("profile_digest", "TEXT", true, None, 0),
        ],
        vec![
            Shape::new("generation_id", "TEXT", true, None, 1),
            Shape::new("pool_id", "TEXT", true, None, 2),
            Shape::new("medium", "TEXT", true, None, 0),
            Shape::new("inode_policy", "TEXT", true, None, 0),
            Shape::new("max_bytes", "INTEGER", true, None, 0),
            Shape::new("max_inodes", "INTEGER", true, None, 0),
            Shape::new("reserve_bytes", "INTEGER", true, None, 0),
            Shape::new("reserve_inodes", "INTEGER", true, None, 0),
        ],
        vec![
            Shape::new("generation_id", "TEXT", true, None, 1),
            Shape::new("role", "TEXT", true, None, 2),
            Shape::new("pool_id", "TEXT", true, None, 0),
            Shape::new("medium", "TEXT", true, None, 0),
            Shape::new("max_bytes", "INTEGER", true, None, 0),
            Shape::new("max_inodes", "INTEGER", true, None, 0),
            Shape::new("retained_memory_bytes", "INTEGER", true, None, 0),
            Shape::new("alias_of", "TEXT", false, None, 0),
        ],
        vec![
            Shape::new("singleton", "INTEGER", false, None, 1),
            Shape::new("generation_id", "TEXT", true, None, 0),
        ],
        vec![
            Shape::new("reservation_id", "INTEGER", true, None, 1),
            Shape::new("profile_generation_id", "TEXT", true, None, 0),
            Shape::new("profile_digest", "TEXT", true, None, 0),
            Shape::new("instance_binding_digest", "TEXT", true, None, 0),
            Shape::new("retained_memory_bytes", "INTEGER", true, None, 0),
        ],
        vec![
            Shape::new("reservation_id", "INTEGER", true, None, 1),
            Shape::new("role", "TEXT", true, None, 2),
            Shape::new("pool_id", "TEXT", true, None, 0),
            Shape::new("filesystem_instance_digest", "TEXT", true, None, 0),
            Shape::new("medium", "TEXT", true, None, 0),
            Shape::new("inode_policy", "TEXT", true, None, 0),
            Shape::new("filesystem_kind", "TEXT", true, None, 0),
            Shape::new("charge_bytes", "INTEGER", true, None, 0),
            Shape::new("charge_inodes", "INTEGER", true, None, 0),
            Shape::new("retained_memory_bytes", "INTEGER", true, None, 0),
        ],
        vec![
            Shape::new("reservation_id", "INTEGER", true, None, 1),
            Shape::new("sample_generation_id", "TEXT", true, None, 2),
            Shape::new("profile_generation_id", "TEXT", true, None, 0),
            Shape::new("profile_digest", "TEXT", true, None, 0),
            Shape::new("engine_id", "TEXT", true, None, 0),
            Shape::new("docker_root_digest", "TEXT", true, None, 0),
            Shape::new("observed_at_unix_ms", "INTEGER", true, None, 0),
            Shape::new("guest_available_bytes", "INTEGER", false, None, 0),
            Shape::new("ancestor_memory_max_bytes", "INTEGER", false, None, 0),
            Shape::new("ancestor_memory_current_bytes", "INTEGER", false, None, 0),
            Shape::new("ancestor_swap_max_bytes", "INTEGER", false, None, 0),
            Shape::new("ancestor_swap_current_bytes", "INTEGER", false, None, 0),
        ],
        vec![
            Shape::new("reservation_id", "INTEGER", true, None, 1),
            Shape::new("sample_generation_id", "TEXT", true, None, 2),
            Shape::new("pool_id", "TEXT", true, None, 3),
            Shape::new("filesystem_instance_digest", "TEXT", true, None, 0),
            Shape::new("medium", "TEXT", true, None, 0),
            Shape::new("filesystem_kind", "TEXT", true, None, 0),
            Shape::new("inode_policy", "TEXT", true, None, 0),
            Shape::new("total_bytes", "INTEGER", true, None, 0),
            Shape::new("available_bytes", "INTEGER", true, None, 0),
            Shape::new("total_inodes", "INTEGER", true, None, 0),
            Shape::new("available_inodes", "INTEGER", true, None, 0),
        ],
    ]
}

fn required_sql() -> [Vec<&'static str>; 8] {
    [
        vec!["primary key", "profile_digest text not null"],
        vec![
            "references host_storage_profiles",
            "inode_policy in ('finite', 'dynamic-btrfs', 'unknown')",
            "primary key (generation_id, pool_id)",
        ],
        vec![
            "references host_storage_profiles",
            "max_inodes > 0",
            "foreign key (generation_id, alias_of) references host_storage_profile_roles",
            "alias_of = 'runner-work' and role in ('cache-live', 'cache-stage')",
            "primary key (generation_id, role)",
            "foreign key (generation_id, pool_id)",
        ],
        vec!["check (singleton = 1)", "references host_storage_profiles"],
        vec![
            "references capacity_reservations",
            "references host_storage_profiles",
            "primary key",
        ],
        vec![
            "references capacity_reservation_storage",
            "inode_policy in ('finite', 'dynamic-btrfs')",
            "primary key (reservation_id, role)",
            "charge_bytes > 0",
        ],
        vec![
            "references capacity_reservation_storage",
            "primary key (reservation_id, sample_generation_id)",
            "observed_at_unix_ms > 0",
            "ancestor_memory_current_bytes <= ancestor_memory_max_bytes",
        ],
        vec![
            "references capacity_reservation_storage_samples",
            "inode_policy in ('finite', 'dynamic-btrfs')",
            "primary key (reservation_id, sample_generation_id, pool_id)",
        ],
    ]
}

struct Shape {
    name: &'static str,
    kind: &'static str,
    not_null: bool,
    default: Option<&'static str>,
    primary_key: i64,
}

impl Shape {
    const fn new(
        name: &'static str,
        kind: &'static str,
        not_null: bool,
        default: Option<&'static str>,
        primary_key: i64,
    ) -> Self {
        Self {
            name,
            kind,
            not_null,
            default,
            primary_key,
        }
    }
}
