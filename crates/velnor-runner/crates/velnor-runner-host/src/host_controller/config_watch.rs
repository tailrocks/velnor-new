//! Read the host config with a size limit and without following links.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::config::HostConfig;
use crate::error::HostError;

const MAX_CONFIG_BYTES: u64 = 1_048_576;

/// Read one stable config snapshot. Missing files leave the last accepted view intact.
pub(super) fn read_host_config(path: &Path) -> Result<Option<HostConfig>, HostError> {
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(HostError::Config),
    };
    let before = file.metadata().map_err(|_| HostError::Config)?;
    if !before.file_type().is_file() || before.len() > MAX_CONFIG_BYTES {
        return Err(HostError::Config);
    }

    let first = read_snapshot(&mut file)?;
    let second = read_snapshot(&mut file)?;
    let after = file.metadata().map_err(|_| HostError::Config)?;
    if before.len() != after.len()
        || u64::try_from(first.len()).map_err(|_| HostError::Config)? != after.len()
        || first != second
    {
        return Err(HostError::Config);
    }
    let text = std::str::from_utf8(&first).map_err(|_| HostError::Config)?;
    HostConfig::parse(text).map(Some)
}

fn read_snapshot(file: &mut File) -> Result<Vec<u8>, HostError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Config)?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if u64::try_from(bytes.len()).map_err(|_| HostError::Config)? > MAX_CONFIG_BYTES {
        return Err(HostError::Config);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::config_test_support::{basic_connection, host_config};

    use super::read_host_config;

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn bounded_config_reads_a_regular_file_and_rejects_a_symlink() -> Result<(), String> {
        let root = test_dir()?;
        let path = root.join("host.toml");
        let alias = root.join("host-link.toml");
        let text = host_config(
            &[basic_connection(
                "essential",
                101,
                "donbeave/essential-mac",
                "essential",
                "essential-scale-set",
            )],
            1,
        );
        fs::write(&path, text).map_err(|error| error.to_string())?;
        let config = read_host_config(&path)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "config file was treated as missing".to_owned())?;
        if config.connections().len() != 1 {
            return Err("config did not keep its connection".to_owned());
        }
        std::os::unix::fs::symlink(&path, &alias).map_err(|error| error.to_string())?;
        if read_host_config(&alias).is_ok() {
            return Err("config symlink was accepted".to_owned());
        }
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn oversized_config_is_rejected_before_parse() -> Result<(), String> {
        let root = test_dir()?;
        let path = root.join("host.toml");
        fs::write(&path, vec![b'x'; 1_048_577]).map_err(|error| error.to_string())?;
        if read_host_config(&path).is_ok() {
            return Err("oversized config was accepted".to_owned());
        }
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn test_dir() -> Result<PathBuf, String> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-host-config-watch-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(root)
    }
}
