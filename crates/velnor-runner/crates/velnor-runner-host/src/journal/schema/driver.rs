use crate::error::HostError;

pub(super) async fn migrate_to_current(
    conn: &turso::Connection,
    mut version: i64,
) -> Result<(), HostError> {
    while version != super::JOURNAL_VERSION {
        let expected = expected_next_version(version).ok_or(HostError::Journal)?;
        match version {
            0 => super::migration::from_version_zero(conn).await?,
            1 => super::migration::from_version_one(conn).await?,
            2 => super::migration::from_version_two(conn).await?,
            3 => super::migration::from_version_three(conn).await?,
            4 => super::migration::from_version_four(conn).await?,
            5 => super::migration::from_version_five(conn).await?,
            6 => super::migration::from_version_six(conn).await?,
            7 => super::migration::from_version_seven(conn).await?,
            8 => super::migration::from_version_eight(conn).await?,
            9 => super::migration::from_version_nine(conn).await?,
            10 => super::migration::from_version_ten(conn).await?,
            11 => super::migration::from_version_eleven(conn).await?,
            12 => super::migration::from_version_twelve(conn).await?,
            13 => super::migration::from_version_thirteen(conn).await?,
            14 => super::migration::from_version_fourteen(conn).await?,
            15 => super::migration::from_version_fifteen(conn).await?,
            16 => super::migration::from_version_sixteen(conn).await?,
            17 => super::migration::from_version_seventeen(conn).await?,
            18 => super::migration::from_version_eighteen(conn).await?,
            19 => super::migration::from_version_nineteen(conn).await?,
            20 => super::migration::from_version_twenty(conn).await?,
            21 => super::migration::from_version_twenty_one(conn).await?,
            22 => super::migration::from_version_twenty_two(conn).await?,
            23 => super::migration::from_version_twenty_three(conn).await?,
            24 => super::migration::from_version_twenty_four(conn).await?,
            25 => super::migration::from_version_twenty_five(conn).await?,
            26 => super::migration::from_version_twenty_six(conn).await?,
            27 => super::migration::from_version_twenty_seven(conn).await?,
            28 => super::migration::from_version_twenty_eight(conn).await?,
            29 => super::migration::from_version_twenty_nine(conn).await?,
            30 => super::migration::from_version_thirty(conn).await?,
            31 => super::migration::from_version_thirty_one(conn).await?,
            32 => super::migration::from_version_thirty_two(conn).await?,
            33 => super::migration::from_version_thirty_three(conn).await?,
            34 => super::migration::from_version_thirty_four(conn).await?,
            35 => super::migration::from_version_thirty_five(conn).await?,
            36 => super::migration::from_version_thirty_six(conn).await?,
            _ => return Err(HostError::Journal),
        }
        version = super::journal_version(conn).await?;
        if version != expected {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

pub(super) const fn expected_next_version(version: i64) -> Option<i64> {
    match version {
        0..=8 | 10..=36 => Some(version + 1),
        9 => Some(11),
        _ => None,
    }
}
