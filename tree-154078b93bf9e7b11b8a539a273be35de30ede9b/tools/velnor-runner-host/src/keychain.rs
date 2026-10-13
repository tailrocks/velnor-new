//! Stdin token stored with the macOS Security framework.
//!
//! Non-macOS imports fail. The token is not an argument, a log line, or an error payload.

use std::io::Read;

use zeroize::Zeroizing;

use crate::HostError;

const MAX_SECRET_LEN: usize = 4096;

/// Read a token from `reader`.
///
/// The returned buffer is wiped on drop. A trailing newline is kept.
///
/// # Errors
///
/// Returns [`HostError::EmptySecret`] when `reader` yields no bytes.
/// Returns [`HostError::Keychain`] when the read fails or yields more than 4096 bytes.
pub fn read_secret<R: Read>(reader: &mut R) -> Result<Zeroizing<Vec<u8>>, HostError> {
    let mut secret = Zeroizing::new(Vec::new());
    let mut limited = reader.take(read_limit());
    limited
        .read_to_end(&mut secret)
        .map_err(|_| HostError::Keychain)?;
    if secret.len() > MAX_SECRET_LEN {
        return Err(HostError::Keychain);
    }
    if secret.is_empty() {
        return Err(HostError::EmptySecret);
    }
    Ok(secret)
}

/// Store `secret` under `service` and `account`.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the store fails. Non-macOS always fails.
pub fn import_secret(service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
    store(service, account, secret)
}

/// Load the token stored under `service` and `account`.
///
/// The buffer is wiped on drop. It is not logged.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the item is missing or the store fails.
/// Non-macOS always fails.
pub fn load_secret(service: &str, account: &str) -> Result<Zeroizing<Vec<u8>>, HostError> {
    Ok(Zeroizing::new(fetch(service, account)?))
}

#[cfg(target_os = "macos")]
fn store(service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
    security_framework::passwords::set_generic_password(service, account, secret)
        .map_err(|_| HostError::Keychain)
}

#[cfg(not(target_os = "macos"))]
fn store(service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
    let _kept = (service.len(), account.len(), secret.len());
    Err(HostError::Keychain)
}

#[cfg(target_os = "macos")]
fn fetch(service: &str, account: &str) -> Result<Vec<u8>, HostError> {
    security_framework::passwords::generic_password(
        security_framework::passwords::PasswordOptions::new_generic_password(service, account),
    )
    .map_err(|_| HostError::Keychain)
}

#[cfg(not(target_os = "macos"))]
fn fetch(service: &str, account: &str) -> Result<Vec<u8>, HostError> {
    let _kept = (service.len(), account.len());
    Err(HostError::Keychain)
}

fn read_limit() -> u64 {
    match u64::try_from(MAX_SECRET_LEN) {
        Ok(len) => len.saturating_add(1),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests;
