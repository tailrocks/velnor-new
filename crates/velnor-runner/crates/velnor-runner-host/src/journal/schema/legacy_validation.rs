//! Strict validation chains for historical journal schema versions.

use crate::error::HostError;

use super::{
    cleanup_observer_witness, connection, doctor_probe, execution_authorization,
    execution_publication, image_import, jit_conflict_recovery, physical_attempts,
    post_jit_effect_fences, post_jit_retirement, pr_source_transition, pre_jit_cleanup,
    provisioning, remote, request_ledger, request_reservations, scale_set_create, session,
    session_refresh, source_authorization, storage_budget, storage_handoff, storage_initialization,
    storage_network, storage_root_backing,
};

macro_rules! validate_v40_stage {
    ($_name:literal, $future:expr) => {{
        $future.await?;
    }};
}

pub(in crate::journal) async fn validate_persisted_schema_v39(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v38(conn).await?;
    jit_conflict_recovery::validate(conn).await
}

/// Compose the pre-v40 schema contracts while selecting only the v40 JIT
/// ledger and terminal-proof contracts.
pub(super) async fn validate_persisted_schema_v40_base(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v38_current40(conn).await?;
    jit_conflict_recovery::validate(conn).await
}

async fn validate_persisted_schema_v38_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v40_stage!(
        "v37 current40",
        validate_persisted_schema_v37_current40(conn)
    );
    validate_v40_stage!("pr source transition", pr_source_transition::validate(conn));
    validate_v40_stage!(
        "pr source rows",
        crate::journal::pr_source_transition::validate_persisted_rows(conn)
    );
    Ok(())
}

async fn validate_persisted_schema_v37_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v40_stage!(
        "v32 core current40",
        validate_persisted_schema_v32_core_current40(conn)
    );
    validate_v40_stage!("storage network v33", storage_network::validate_v33(conn));
    validate_v40_stage!(
        "physical attempts v35",
        physical_attempts::validate_v35(conn)
    );
    validate_v40_stage!(
        "post JIT effect fences",
        post_jit_effect_fences::validate(conn)
    );
    validate_v40_stage!(
        "post JIT retirement v40",
        post_jit_retirement::validate_v40(conn)
    );
    Ok(())
}

async fn validate_persisted_schema_v32_core_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v40_stage!(
        "v27 core current40",
        validate_persisted_schema_v27_core_current40(conn)
    );
    validate_v40_stage!("storage root backing", storage_root_backing::validate(conn));
    validate_v40_stage!(
        "execution publication v30",
        execution_publication::validate_v30(conn)
    );
    validate_v40_stage!(
        "cleanup observer witness",
        cleanup_observer_witness::validate_current(conn)
    );
    validate_v40_stage!("pre JIT cleanup", pre_jit_cleanup::validate(conn));
    Ok(())
}

async fn validate_persisted_schema_v27_core_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v40_stage!(
        "v20 current40",
        validate_persisted_schema_v20_current40(conn)
    );
    validate_v40_stage!(
        "storage initialization v22",
        storage_initialization::validate_v22(conn)
    );
    validate_v40_stage!(
        "execution authorization",
        execution_authorization::validate(conn)
    );
    validate_v40_stage!(
        "execution publication",
        execution_publication::validate(conn)
    );
    validate_v40_stage!(
        "execution publication v27",
        execution_publication::validate_v27(conn)
    );
    validate_v40_stage!("storage handoff", storage_handoff::validate(conn));
    Ok(())
}

async fn validate_persisted_schema_v20_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v40_stage!(
        "v19 current40",
        validate_persisted_schema_v19_current40(conn)
    );
    validate_v40_stage!(
        "storage initialization",
        storage_initialization::validate(conn)
    );
    validate_v40_stage!(
        "v20 revision triggers",
        crate::journal_schema::validate_v20_revision_triggers(conn)
    );
    Ok(())
}

pub(super) async fn validate_persisted_schema_v19_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v40_stage!(
        "v17 current40",
        validate_persisted_schema_v17_current40(conn)
    );
    validate_v40_stage!("doctor probe", doctor_probe::validate(conn));
    validate_v40_stage!("request reservations", request_reservations::validate(conn));
    Ok(())
}

async fn validate_persisted_schema_v17_current40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let validations = [
        ("current", super::validate_current_schema(conn).await),
        (
            "connection",
            connection::validate_connection_schema(conn).await,
        ),
        ("remote", remote::validate(conn).await),
        ("image_import", image_import::validate(conn).await),
        ("scale_set_create", scale_set_create::validate(conn).await),
        (
            "source_authorization",
            source_authorization::validate(conn).await,
        ),
        (
            "request_ledger_v40",
            request_ledger::validate_v40(conn).await,
        ),
        (
            "probe_lease",
            crate::journal_schema::validate_probe_lease(conn).await,
        ),
        (
            "capacity_header",
            crate::journal::capacity_header::validate_connection(conn).await,
        ),
        ("provisioning", provisioning::validate_v40(conn).await),
        ("session", session::validate(conn).await),
        ("session_refresh", session_refresh::validate(conn).await),
        ("storage_budget", storage_budget::validate(conn).await),
    ];
    for (_name, result) in validations {
        result?;
    }
    Ok(())
}

pub(in crate::journal) async fn validate_persisted_schema_v36(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    super::validate_persisted_schema_v35(conn).await?;
    post_jit_effect_fences::validate(conn).await
}

pub(in crate::journal) async fn validate_persisted_schema_v37(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v32_core(conn).await?;
    storage_network::validate_v33(conn).await?;
    physical_attempts::validate_v35(conn).await?;
    post_jit_effect_fences::validate(conn).await?;
    post_jit_retirement::validate_v37(conn).await
}

pub(in crate::journal) async fn validate_persisted_schema_v38(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v37(conn).await?;
    pr_source_transition::validate(conn).await?;
    crate::journal::pr_source_transition::validate_persisted_rows(conn).await
}

pub(in crate::journal) async fn validate_persisted_schema_v33(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v32_core(conn).await?;
    storage_network::validate_v33(conn).await?;
    physical_attempts::validate_v33(conn).await
}

pub(in crate::journal) async fn validate_persisted_schema_v34(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v32_core(conn).await?;
    storage_network::validate_v33(conn).await?;
    physical_attempts::validate(conn).await
}

pub(in crate::journal) async fn validate_persisted_schema_v32(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    storage_network::validate(conn).await?;
    validate_persisted_schema_v32_core(conn).await
}

pub(super) async fn validate_persisted_schema_v32_core(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v27_core(conn).await?;
    storage_root_backing::validate(conn).await?;
    execution_publication::validate_v30(conn).await?;
    cleanup_observer_witness::validate_current(conn).await?;
    pre_jit_cleanup::validate(conn).await
}

pub(in crate::journal) async fn validate_persisted_schema_v27(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v27_core(conn).await?;
    storage_network::validate(conn).await?;
    storage_root_backing::validate_v27(conn).await
}

async fn validate_persisted_schema_v27_core(conn: &turso::Connection) -> Result<(), HostError> {
    validate_persisted_schema_v20(conn).await?;
    storage_initialization::validate_v22(conn).await?;
    execution_authorization::validate(conn).await?;
    execution_publication::validate(conn).await?;
    execution_publication::validate_v27(conn).await?;
    storage_handoff::validate(conn).await?;
    Ok(())
}

pub(in crate::journal) async fn validate_persisted_schema_v20(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v19(conn).await?;
    storage_initialization::validate(conn).await?;
    crate::journal_schema::validate_v20_revision_triggers(conn).await?;
    Ok(())
}

pub(in crate::journal) async fn validate_persisted_schema_v19(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v17(conn).await?;
    doctor_probe::validate(conn).await?;
    request_reservations::validate(conn).await
}

pub(super) async fn validate_persisted_schema_v17(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let validations = [
        ("current", super::validate_current_schema(conn).await),
        (
            "connection",
            connection::validate_connection_schema(conn).await,
        ),
        ("remote", remote::validate(conn).await),
        ("image_import", image_import::validate(conn).await),
        ("scale_set_create", scale_set_create::validate(conn).await),
        (
            "source_authorization",
            source_authorization::validate(conn).await,
        ),
        ("request_ledger", request_ledger::validate(conn).await),
        (
            "probe_lease",
            crate::journal_schema::validate_probe_lease(conn).await,
        ),
        (
            "capacity_header",
            crate::journal::capacity_header::validate_connection(conn).await,
        ),
        ("provisioning", provisioning::validate(conn).await),
        ("session", session::validate(conn).await),
        ("session_refresh", session_refresh::validate(conn).await),
        ("storage_budget", storage_budget::validate(conn).await),
    ];
    for (_, result) in validations {
        result?;
    }
    Ok(())
}
