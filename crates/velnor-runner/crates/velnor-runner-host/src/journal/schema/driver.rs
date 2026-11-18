// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

pub(super) async fn migrate_to_current(
    conn: &turso::Connection,
    mut version: i64,
) -> Result<(), HostError> {
    while version != super::JOURNAL_VERSION {
        let expected = expected_next_version(version).ok_or(HostError::Journal)?;
        let step = migration_step(conn, version)?;
        step.await?;
        version = super::journal_version(conn).await?;
        if version != expected {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

type MigrationStep<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), HostError>> + Send + 'a>>;

#[inline(never)]
fn migration_step(conn: &turso::Connection, version: i64) -> Result<MigrationStep<'_>, HostError> {
    let step: MigrationStep<'_> = match version {
        0 => Box::pin(super::migration::from_version_zero(conn)),
        1 => Box::pin(super::migration::from_version_one(conn)),
        2 => Box::pin(super::migration::from_version_two(conn)),
        3 => Box::pin(super::migration::from_version_three(conn)),
        4 => Box::pin(super::migration::from_version_four(conn)),
        5 => Box::pin(super::migration::from_version_five(conn)),
        6 => Box::pin(super::migration::from_version_six(conn)),
        7 => Box::pin(super::migration::from_version_seven(conn)),
        8 => Box::pin(super::migration::from_version_eight(conn)),
        9 => Box::pin(super::migration::from_version_nine(conn)),
        10 => Box::pin(super::migration::from_version_ten(conn)),
        11 => Box::pin(super::migration::from_version_eleven(conn)),
        12 => Box::pin(super::migration::from_version_twelve(conn)),
        13 => Box::pin(super::migration::from_version_thirteen(conn)),
        14 => Box::pin(super::migration::from_version_fourteen(conn)),
        15 => Box::pin(super::migration::from_version_fifteen(conn)),
        16 => Box::pin(super::migration::from_version_sixteen(conn)),
        17 => Box::pin(super::migration::from_version_seventeen(conn)),
        18 => Box::pin(super::migration::from_version_eighteen(conn)),
        19 => Box::pin(super::migration::from_version_nineteen(conn)),
        20 => Box::pin(super::migration::from_version_twenty(conn)),
        21 => Box::pin(super::migration::from_version_twenty_one(conn)),
        22 => Box::pin(super::migration::from_version_twenty_two(conn)),
        23 => Box::pin(super::migration::from_version_twenty_three(conn)),
        24 => Box::pin(super::migration::from_version_twenty_four(conn)),
        25 => Box::pin(super::migration::from_version_twenty_five(conn)),
        26 => Box::pin(super::migration::from_version_twenty_six(conn)),
        27 => Box::pin(super::migration::from_version_twenty_seven(conn)),
        28 => Box::pin(super::migration::from_version_twenty_eight(conn)),
        29 => Box::pin(super::migration::from_version_twenty_nine(conn)),
        30 => Box::pin(super::migration::from_version_thirty(conn)),
        31 => Box::pin(super::migration::from_version_thirty_one(conn)),
        32 => Box::pin(super::migration::from_version_thirty_two(conn)),
        33 => Box::pin(super::migration::from_version_thirty_three(conn)),
        34 => Box::pin(super::migration::from_version_thirty_four(conn)),
        35 => Box::pin(super::migration::from_version_thirty_five(conn)),
        36 => Box::pin(super::migration::from_version_thirty_six(conn)),
        37 => Box::pin(super::migration::from_version_thirty_seven(conn)),
        38 => Box::pin(super::migration::from_version_thirty_eight(conn)),
        39 => Box::pin(super::jit_conflict_settlement::migrate_v39_to_v40(conn)),
        40 => Box::pin(super::worker_health::migrate_v40_to_v41(conn)),
        41 => Box::pin(super::registry_image_import::migrate_v41_to_v42(conn)),
        42 => Box::pin(super::unbound_retirement::migrate_v42_to_v43(conn)),
        43 => Box::pin(super::unbound_retirement::migrate_v43_to_v44(conn)),
        44 => Box::pin(super::unbound_retirement::migrate_v44_to_v45(conn)),
        45 => Box::pin(super::jit_interrupted_claims::migrate_v45_to_v46(conn)),
        46 => Box::pin(super::jit_interrupted_claims::migrate_v46_to_v47(conn)),
        47 => Box::pin(super::jit_interrupted_claims::migrate_v47_to_v48(conn)),
        48 => Box::pin(super::jit_claim_deadline::migrate(conn)),
        49 => Box::pin(super::jit_tombstone_completion::migrate(conn)),
        50 => Box::pin(super::unbound_retirement::migrate_v50_to_v51(conn)),
        51 => Box::pin(super::unbound_retirement::migrate_v51_to_v52(conn)),
        // TEMPORARY BRIDGE for #1075: schema 53-55 (#1277, #1280, #1283) are not on main yet,
        // so main's v52 jumps straight to 56. Before merging #1075, rebase onto #1283's chain:
        // replace this arm with `55 => ...migrate_v55_to_v56` and make expected_next_version
        // treat 52..=54 as version+1 again.
        52 => Box::pin(super::unbound_retirement::migrate_v55_to_v56(conn)),
        _ => return Err(HostError::Journal),
    };
    Ok(step)
}

pub(super) const fn expected_next_version(version: i64) -> Option<i64> {
    match version {
        0..=8 | 10..=51 => Some(version + 1),
        9 => Some(11),
        // TEMPORARY BRIDGE for #1075: see migration_step arm 52.
        52 => Some(56),
        _ => None,
    }
}

#[cfg(test)]
pub(in crate::journal) async fn bootstrap_historical_v45(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let mut version = super::journal_version(conn).await?;
    while version != 45 {
        let expected = expected_next_version(version).ok_or(HostError::Journal)?;
        migration_step(conn, version)?.await?;
        version = super::journal_version(conn).await?;
        if version != expected {
            return Err(HostError::Journal);
        }
    }
    super::validate_persisted_schema_v45(conn).await
}

#[cfg(test)]
pub(in crate::journal) async fn bootstrap_historical_v47(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let mut version = super::journal_version(conn).await?;
    while version != 47 {
        let expected = expected_next_version(version).ok_or(HostError::Journal)?;
        migration_step(conn, version)?.await?;
        version = super::journal_version(conn).await?;
        if version != expected {
            return Err(HostError::Journal);
        }
    }
    super::validate_persisted_schema_v46(conn).await
}

#[cfg(test)]
pub(in crate::journal) async fn bootstrap_historical_v48(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let mut version = super::journal_version(conn).await?;
    while version != 48 {
        let expected = expected_next_version(version).ok_or(HostError::Journal)?;
        migration_step(conn, version)?.await?;
        version = super::journal_version(conn).await?;
        if version != expected {
            return Err(HostError::Journal);
        }
    }
    super::validate_persisted_schema_v46(conn).await?;
    super::jit_interrupted_claims::validate_initial_absence(conn).await
}

#[cfg(test)]
pub(in crate::journal) async fn bootstrap_historical_v46(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    bootstrap_historical_v45(conn).await?;
    super::jit_interrupted_claims::migrate_v45_to_v46(conn).await?;
    super::validate_persisted_schema_v45(conn).await?;
    super::jit_interrupted_claims::validate(conn).await
}
