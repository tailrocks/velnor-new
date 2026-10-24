//! Version-40 schema support for charged JIT conflict retirement.

use crate::error::HostError;

mod ddl;
mod guards;
mod migration;
mod validation;

#[cfg(test)]
#[path = "jit_conflict_settlement/tests.rs"]
mod tests;

#[cfg(test)]
pub(in crate::journal::schema) use tests::downgrade_current_database_to_v39;

pub(super) async fn migrate_v39_to_v40(conn: &turso::Connection) -> Result<(), HostError> {
    migration::migrate_v39_to_v40(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate(conn).await
}

pub(super) async fn validate_persisted_proofs(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_persisted_jit_conflict_proofs(conn).await
}

pub(super) fn metadata_claim_guard_sql_v40() -> Result<String, HostError> {
    guards::metadata_claim_guard_sql_v40()
}

pub(super) fn jit_table_sql() -> &'static str {
    ddl::JIT_TABLE
}

pub(super) fn jit_index_contracts() -> &'static [(&'static str, &'static str); 5] {
    &ddl::JIT_INDEXES
}

pub(super) fn terminal_view_sql() -> &'static str {
    ddl::TERMINAL_PROOFS_VIEW
}

pub(super) fn table_names() -> &'static [&'static str; 3] {
    &ddl::TABLE_NAMES
}

pub(super) fn schema_objects() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    ddl::TABLE_NAMES
        .into_iter()
        .zip([ddl::CLAIMS, ddl::PROGRESS, ddl::SETTLEMENTS])
        .map(|(name, sql)| ("table", name, sql))
        .chain(
            guards::INSERT_GUARDS
                .into_iter()
                .map(|sql| ("trigger", trigger_name(sql), sql)),
        )
        .chain(
            guards::IMMUTABLE_GUARDS
                .into_iter()
                .map(|sql| ("trigger", trigger_name(sql), sql)),
        )
        .chain(
            guards::EFFECT_FENCES
                .into_iter()
                .map(|sql| ("trigger", trigger_name(sql), sql)),
        )
}

pub(super) fn revision_trigger_sql(table: &str, suffix: &str, action: &str) -> String {
    format!(
        "CREATE TRIGGER {table}_revision_{suffix} {action} ON {table} BEGIN UPDATE journal_meta SET revision=revision+1 WHERE singleton=1; END"
    )
}

fn trigger_name(sql: &'static str) -> &'static str {
    let body = sql.strip_prefix("CREATE TRIGGER ").unwrap_or(sql);
    // All names are static literals in `guards`; parsing only selects their first token.
    let name = body.split_ascii_whitespace().next().unwrap_or("");
    match name {
        "physical_attempt_jit_conflict_claim_insert_guard"
        | "physical_attempt_jit_conflict_claim_no_update"
        | "physical_attempt_jit_conflict_claim_no_delete"
        | "physical_attempt_jit_conflict_progress_insert_guard"
        | "physical_attempt_jit_conflict_progress_no_update"
        | "physical_attempt_jit_conflict_progress_no_delete"
        | "physical_attempt_jit_conflict_settlement_insert_guard"
        | "physical_attempt_jit_conflict_settlement_no_update"
        | "physical_attempt_jit_conflict_settlement_no_delete"
        | "jit_conflict_metadata_claim_proof_guard_recovery_core"
        | "jit_conflict_metadata_claim_proof_guard_recovery_detail"
        | "jit_conflict_metadata_claim_proof_guard_remote_history"
        | "jit_conflict_metadata_claim_proof_guard_remote_no_events"
        | "jit_conflict_jit_insert_fence"
        | "jit_conflict_jit_start_fence"
        | "jit_conflict_provisioning_jit_fence"
        | "jit_conflict_publication_fence"
        | "jit_conflict_delivery_fence"
        | "jit_conflict_challenge_fence"
        | "jit_conflict_decision_fence"
        | "jit_conflict_publication_insert_fence" => name,
        _ => "invalid_jit_conflict_trigger_name",
    }
}
