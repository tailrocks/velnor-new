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
        _ => return Err(HostError::Journal),
    };
    Ok(step)
}

pub(super) const fn expected_next_version(version: i64) -> Option<i64> {
    match version {
        0..=8 | 10..=39 => Some(version + 1),
        9 => Some(11),
        _ => None,
    }
}
