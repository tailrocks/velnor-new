use super::fixtures::scalar;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::request_lifecycle::tests::helpers::Scratch;

#[tokio::test]
async fn v40_validation_rejects_each_jit_index_and_restores_exact_contract() -> Result<(), HostError>
{
    Box::pin(v40_validation_rejects_each_jit_index_and_restores_exact_contract_inner()).await
}

async fn v40_validation_rejects_each_jit_index_and_restores_exact_contract_inner()
-> Result<(), HostError> {
    let (_scratch, journal) = opened_v40().await?;
    let connection = journal.connection().await?;
    super::super::super::validate_persisted_schema_v40(&connection).await?;
    for (name, _) in super::super::jit_index_contracts() {
        let sql = object_sql(&connection, "index", name).await?;
        drop_object(&connection, "INDEX", name).await?;
        assert!(schema_is_invalid(&connection).await);
        connection
            .execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
        super::super::super::validate_persisted_schema_v40(&connection).await?;
    }
    Ok(())
}

#[tokio::test]
async fn v40_validation_rejects_each_old_and_new_recovery_guard() -> Result<(), HostError> {
    Box::pin(v40_validation_rejects_each_old_and_new_recovery_guard_inner()).await
}

async fn v40_validation_rejects_each_old_and_new_recovery_guard_inner() -> Result<(), HostError> {
    let (_scratch, journal) = opened_v40().await?;
    let connection = journal.connection().await?;
    super::super::super::validate_persisted_schema_v40(&connection).await?;

    let legacy = prefixed_sql(&connection, "post_jit_").await?;
    assert_eq!(legacy.len(), 8);
    for (name, sql) in legacy {
        drop_object(&connection, "TRIGGER", &name).await?;
        assert!(schema_is_invalid(&connection).await);
        connection
            .execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
        super::super::super::validate_persisted_schema_v40(&connection).await?;
    }

    let guards = super::super::schema_objects()
        .filter(|(kind, _, _)| *kind == "trigger")
        .map(|(_, name, sql)| (name.to_owned(), sql.to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(guards.len(), 21);
    for suffix in [
        "recovery_core",
        "recovery_detail",
        "remote_history",
        "remote_no_events",
    ] {
        let expected = format!("jit_conflict_metadata_claim_proof_guard_{suffix}");
        assert!(guards.iter().any(|(name, _)| name == &expected));
    }
    for (name, sql) in guards {
        drop_object(&connection, "TRIGGER", &name).await?;
        assert!(schema_is_invalid(&connection).await);
        connection
            .execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
        super::super::super::validate_persisted_schema_v40(&connection).await?;
    }

    for table in super::super::table_names() {
        for suffix in ["insert", "update", "delete"] {
            let name = format!("{table}_revision_{suffix}");
            let sql = super::super::revision_trigger_sql(
                table,
                suffix,
                match suffix {
                    "insert" => "AFTER INSERT",
                    "update" => "AFTER UPDATE",
                    _ => "AFTER DELETE",
                },
            );
            drop_object(&connection, "TRIGGER", &name).await?;
            assert!(schema_is_invalid(&connection).await);
            connection
                .execute(&sql, ())
                .await
                .map_err(|_| HostError::Journal)?;
            super::super::super::validate_persisted_schema_v40(&connection).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn v40_sidecar_insert_guards_reject_unbacked_rows() -> Result<(), HostError> {
    Box::pin(v40_sidecar_insert_guards_reject_unbacked_rows_inner()).await
}

async fn v40_sidecar_insert_guards_reject_unbacked_rows_inner() -> Result<(), HostError> {
    let (_scratch, journal) = opened_v40().await?;
    let connection = journal.connection().await?;
    let zero32 = "00000000000000000000000000000000";
    let zero64 = "0000000000000000000000000000000000000000000000000000000000000000";
    let requested_name = format!("v{zero32}");
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_claims (reservation_id,claim_generation,lineage_id,generation,intent_id,connection_id,scale_set_id,witness_request_id,physical_nonce,binding_digest,source_digest,acquire_response_digest,authority_generation,authority_digest,engine_id,docker_root_digest,requested_name,snapshot_sha256,lease_until,created_at) VALUES (999999,1,1,1,1,'missing',1,1,?1,?2,?2,?2,1,?2,'test-engine',?2,?3,?2,unixepoch()+60,unixepoch())",
            (zero32, zero64, requested_name.as_str()),
        )
        .await
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (999999,1,1,'local_absent',?1,NULL,NULL,NULL,NULL,NULL,NULL,NULL,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            [zero64],
        )
        .await
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_settlements (lineage_id,generation,reservation_id,claim_generation,metadata_generation,remote_runner_id,remote_runner_name,remote_scale_set_id,proof_digest,settlement_digest,created_at) VALUES (1,1,999999,1,1,1,?1,1,?2,?2,unixepoch())",
            (requested_name.as_str(), zero64),
        )
        .await
        .is_err());
    for table in super::super::table_names() {
        assert_eq!(
            scalar(&connection, &format!("SELECT COUNT(*) FROM {table}")).await?,
            0
        );
    }
    super::super::super::validate_persisted_schema_v40(&connection).await
}

#[tokio::test]
async fn remote_initial_absence_rejects_nonempty_lookup_count() -> Result<(), HostError> {
    let fixture = super::local_absence_fixture('2', 821, 1_121).await?;
    let receipt = super::verified_empty_lookup(&fixture.requested_name)?;
    let response_sha256 = receipt.response_sha256().ok_or(HostError::Journal)?;
    let connection = fixture.journal.connection().await?;
    let evidence = "0".repeat(64);
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,2,'remote_initial_absent',?3,NULL,?4,?5,?6,?7,NULL,200,1,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (fixture.recovery_claim.reservation_id(), fixture.recovery_claim.claim_generation(), evidence.as_str(), fixture.requested_name.as_str(), fixture.recovery_claim.scale_set_id(), receipt.request_sha256(), response_sha256),
        )
        .await
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,2,'remote_initial_absent',?3,NULL,?4,?5,?6,?7,NULL,NULL,NULL,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (fixture.recovery_claim.reservation_id(), fixture.recovery_claim.claim_generation(), evidence.as_str(), fixture.requested_name.as_str(), fixture.recovery_claim.scale_set_id(), receipt.request_sha256(), response_sha256),
        )
        .await
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,2,'remote_initial_absent',?3,NULL,?4,?5,?6,NULL,NULL,200,0,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (fixture.recovery_claim.reservation_id(), fixture.recovery_claim.claim_generation(), evidence.as_str(), fixture.requested_name.as_str(), fixture.recovery_claim.scale_set_id(), receipt.request_sha256()),
        )
        .await
        .is_err());
    fixture
        .scope
        .record_jit_conflict_remote_initial_absence(&fixture.recovery_claim, &receipt)
        .await?;
    assert_eq!(
        scalar(
            &connection,
            &format!("SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id={} AND phase_index=2 AND phase='remote_initial_absent'", fixture.recovery_claim.reservation_id()),
        )
        .await?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn remote_absence_without_same_claim_initial_absence_is_rejected() -> Result<(), HostError> {
    let fixture = super::local_absence_fixture('3', 822, 1_122).await?;
    let receipt = super::verified_empty_lookup(&fixture.requested_name)?;
    let response_sha256 = receipt.response_sha256().ok_or(HostError::Journal)?;
    let connection = fixture.journal.connection().await?;
    let insert_guard = object_sql(
        &connection,
        "trigger",
        "physical_attempt_jit_conflict_progress_insert_guard",
    )
    .await?;
    // Seed an intentionally corrupt predecessor so phase 4 is next but lacks phase 2.
    drop_object(
        &connection,
        "TRIGGER",
        "physical_attempt_jit_conflict_progress_insert_guard",
    )
    .await?;
    let evidence = "0".repeat(64);
    connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,3,'delete_started',?3,1,?4,?5,NULL,NULL,NULL,NULL,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (fixture.recovery_claim.reservation_id(), fixture.recovery_claim.claim_generation(), evidence.as_str(), fixture.requested_name.as_str(), fixture.recovery_claim.scale_set_id()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&insert_guard, ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,4,'remote_absent',?3,NULL,?4,?5,?6,?7,NULL,200,0,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (fixture.recovery_claim.reservation_id(), fixture.recovery_claim.claim_generation(), evidence.as_str(), fixture.requested_name.as_str(), fixture.recovery_claim.scale_set_id(), receipt.request_sha256(), response_sha256),
        )
        .await
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,remove_status,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,4,'remote_absent',?3,NULL,?4,?5,?6,?7,NULL,NULL,NULL,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (fixture.recovery_claim.reservation_id(), fixture.recovery_claim.claim_generation(), evidence.as_str(), fixture.requested_name.as_str(), fixture.recovery_claim.scale_set_id(), receipt.request_sha256(), response_sha256),
        )
        .await
        .is_err());
    assert!(schema_is_invalid(&connection).await);
    Ok(())
}

#[tokio::test]
async fn reopen_rejects_matching_but_noncanonical_settlement_digests() -> Result<(), HostError> {
    let fixture = super::local_absence_fixture('4', 823, 1_123).await?;
    assert_eq!(
        super::settle_empty_remote_case(&fixture, '6').await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    let path = fixture.scratch.file();
    let reservation_id = fixture.physical.reservation_id;
    let super::LocalAbsenceFixture {
        scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    let immutable_guard = object_sql(
        &connection,
        "trigger",
        "physical_attempt_jit_conflict_settlement_no_update",
    )
    .await?;
    // Simulate an offline row edit, restoring the production guard before reopen validation.
    drop_object(
        &connection,
        "TRIGGER",
        "physical_attempt_jit_conflict_settlement_no_update",
    )
    .await?;
    let forged_digest = "0".repeat(64);
    assert_eq!(
        connection
            .execute(
                "UPDATE physical_attempt_jit_conflict_settlements SET proof_digest=?1,settlement_digest=?1 WHERE reservation_id=?2",
                (forged_digest.as_str(), reservation_id),
            )
            .await
            .map_err(|_| HostError::Journal)?,
        1
    );
    connection
        .execute(&immutable_guard, ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn v40_open_rejects_missing_sidecar_and_altered_terminal_view() -> Result<(), HostError> {
    Box::pin(v40_open_rejects_missing_sidecar_and_altered_terminal_view_inner()).await
}

async fn v40_open_rejects_missing_sidecar_and_altered_terminal_view_inner() -> Result<(), HostError>
{
    for tamper in ["sidecar", "view"] {
        let scratch = Scratch::new()?;
        let path = scratch.file();
        {
            let journal = Journal::open(&path).await?;
            let connection = journal.connection().await?;
            if tamper == "sidecar" {
                drop_object(
                    &connection,
                    "TABLE",
                    "physical_attempt_jit_conflict_settlements",
                )
                .await?;
            } else {
                drop_object(&connection, "VIEW", "physical_attempt_terminal_proofs").await?;
                connection
                    .execute(
                        "CREATE VIEW physical_attempt_terminal_proofs AS SELECT lineage_id,generation,reservation_id,outcome,proof_digest,settlement_digest,created_at FROM physical_attempt_outcomes",
                        (),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            assert!(schema_is_invalid(&connection).await);
        }
        assert!(Journal::open(&path).await.is_err());
    }
    Ok(())
}

async fn opened_v40() -> Result<(Scratch, Journal), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    Ok((scratch, journal))
}

async fn object_sql(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2 AND sql IS NOT NULL",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn prefixed_sql(
    connection: &turso::Connection,
    prefix: &str,
) -> Result<Vec<(String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND name GLOB ?1 ORDER BY name",
            (format!("{prefix}*"),),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut objects = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        objects.push((
            row.get(0).map_err(|_| HostError::Journal)?,
            row.get(1).map_err(|_| HostError::Journal)?,
        ));
    }
    Ok(objects)
}

async fn drop_object(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<(), HostError> {
    connection
        .execute(&format!("DROP {kind} {name}"), ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

async fn schema_is_invalid(connection: &turso::Connection) -> bool {
    super::super::super::validate_persisted_schema_v40(connection)
        .await
        .is_err()
}
