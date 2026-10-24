use std::path::Path;

use crate::error::HostError;

const VERSION_EIGHTEEN: &[u8] =
    include_bytes!("../request_lifecycle/tests/v18_topology_migration/v18_fixture.sqlite");

pub(crate) async fn install(path: &Path, target_version: i64) -> Result<(), HostError> {
    if !(18..=31).contains(&target_version) {
        return Err(HostError::Journal);
    }
    std::fs::write(path, VERSION_EIGHTEEN).map_err(|_| HostError::Journal)?;
    let path_text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut version = super::journal_version(&connection).await?;
    if version != 18 {
        return Err(HostError::Journal);
    }
    while version < target_version {
        match version {
            18 => super::migration::from_version_eighteen(&connection).await?,
            19 => super::migration::from_version_nineteen(&connection).await?,
            20 => super::migration::from_version_twenty(&connection).await?,
            21 => super::migration::from_version_twenty_one(&connection).await?,
            22 => super::migration::from_version_twenty_two(&connection).await?,
            23 => super::migration::from_version_twenty_three(&connection).await?,
            24 => super::migration::from_version_twenty_four(&connection).await?,
            25 => super::migration::from_version_twenty_five(&connection).await?,
            26 => super::migration::from_version_twenty_six(&connection).await?,
            27 => super::migration::from_version_twenty_seven(&connection).await?,
            28 => super::migration::from_version_twenty_eight(&connection).await?,
            29 => super::migration::from_version_twenty_nine(&connection).await?,
            30 => super::migration::from_version_thirty(&connection).await?,
            _ => return Err(HostError::Journal),
        }
        version = super::journal_version(&connection).await?;
    }
    if version == target_version {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn installs_version_twenty_from_version_eighteen_fixture() -> Result<(), HostError> {
        let path = std::env::temp_dir().join(format!(
            "velnor-historical-fixture-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| HostError::Journal)?
                .as_nanos()
        ));
        install(&path, 20).await?;
        let path_text = path.to_str().ok_or(HostError::Path)?;
        let database = turso::Builder::new_local(path_text)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        let connection = database.connect().map_err(|_| HostError::Journal)?;
        if super::super::journal_version(&connection).await? != 20 {
            return Err(HostError::Journal);
        }
        drop(connection);
        drop(database);
        let _removed = std::fs::remove_file(path);
        Ok(())
    }

    #[tokio::test]
    async fn upgrades_empty_version_twenty_three_fixture() -> Result<(), HostError> {
        let path = std::env::temp_dir().join(format!(
            "velnor-historical-fixture-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| HostError::Journal)?
                .as_nanos()
        ));
        install(&path, 23).await?;
        drop(crate::journal::Journal::open(&path).await?);
        let _removed = std::fs::remove_file(path);
        Ok(())
    }
}
