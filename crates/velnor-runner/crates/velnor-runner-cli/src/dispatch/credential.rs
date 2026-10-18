use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use velnor_runner_host::{HostError, KeychainReference, import_secret, read_secret};

use super::connect::ConnectError;
use super::connection_state::{config_lock, persist_host, read_host};

const DEFAULT_RELEASE_READ_REFERENCE: &str = "keychain:com.tailrocks.velnor.host/release-read";

pub(super) fn import_release_read(state: &Path) -> ExitCode {
    let mut stdin = std::io::stdin();
    match import_release_read_with(&mut stdin, state, import_secret, persist_host) {
        Ok(()) => {
            println!("Release-read credential imported.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Release-read credential import failed: {error}");
            ExitCode::from(1)
        }
    }
}

pub(super) fn import_release_read_with<R, Store, Persist>(
    input: &mut R,
    state: &Path,
    store: Store,
    persist: Persist,
) -> Result<(), ConnectError>
where
    R: Read,
    Store: FnOnce(&str, &str, &[u8]) -> Result<(), HostError>,
    Persist: FnOnce(&Path, &str) -> Result<(), ConnectError>,
{
    let secret = read_secret(input).map_err(ConnectError::Secret)?;
    std::str::from_utf8(&secret).map_err(|_| ConnectError::Secret(HostError::Keychain))?;
    let _config_lock = config_lock(state).map_err(|_| ConnectError::Lock)?;
    let mut config = read_host(state)?.ok_or(ConnectError::Config)?;
    let (reference, update) = match config.release_read_credential_ref() {
        Some(reference) => (reference.clone(), None),
        None => {
            let reference = KeychainReference::parse(DEFAULT_RELEASE_READ_REFERENCE)
                .map_err(|_| ConnectError::Config)?;
            config
                .set_release_read_credential_ref(reference.clone())
                .map_err(|_| ConnectError::Config)?;
            let text = config.to_toml().map_err(|_| ConnectError::Config)?;
            (reference, Some(text))
        }
    };

    store(reference.service(), reference.account(), &secret).map_err(ConnectError::Secret)?;
    if let Some(text) = update {
        persist(state, &text)?;
    }
    Ok(())
}
