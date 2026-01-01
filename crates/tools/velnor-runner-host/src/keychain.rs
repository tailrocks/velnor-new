//! Host-only credential storage. Values never enter config, argv, or logs.

use std::io::Read;

use zeroize::Zeroizing;

use crate::HostError;

#[cfg(target_os = "linux")]
mod linux;

pub(super) const MAX_SECRET_LEN: usize = 4096;
#[cfg(any(target_os = "macos", test))]
const MACOS_CREDENTIAL_REF: &str = "keychain:com.tailrocks.velnor.host/velnor-host";
#[cfg(target_os = "macos")]
const MACOS_CREDENTIAL_SERVICE: &str = "com.tailrocks.velnor.host";
#[cfg(target_os = "macos")]
const MACOS_CREDENTIAL_ACCOUNT: &str = "velnor-host";
#[cfg(target_os = "linux")]
const ACTIONS_READ_TOKEN_NAME: &str = "actions-read-token";
#[cfg(target_os = "macos")]
const MACOS_ACTIONS_READ_TOKEN_SERVICE: &str = "com.tailrocks.velnor.host";
#[cfg(target_os = "macos")]
const MACOS_ACTIONS_READ_TOKEN_ACCOUNT: &str = "actions-read-token";

/// Read a token from `reader`. The returned buffer is wiped on drop.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] if the input cannot be read or exceeds the
/// accepted size, and [`HostError::EmptySecret`] when no token is supplied.
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

/// Store a caller-supplied credential item. Non-macOS always fails.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the platform cannot store the item or
/// the credential values are not accepted.
pub fn import_secret(service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
    store(service, account, secret)
}

/// Load a caller-supplied macOS Keychain item.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the item cannot be read from the
/// selected platform credential store.
pub fn load_secret(service: &str, account: &str) -> Result<Zeroizing<Vec<u8>>, HostError> {
    Ok(Zeroizing::new(fetch(service, account)?))
}

/// Store the configured host credential without putting its value in config.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the configured host source is
/// unsupported or its ownership, permissions, or write fails.
pub fn store_configured_secret(reference: &str, secret: &[u8]) -> Result<(), HostError> {
    if secret.is_empty() || secret.len() > MAX_SECRET_LEN {
        return Err(HostError::Keychain);
    }
    #[cfg(target_os = "linux")]
    if reference == "systemd-credential:github-token" {
        return linux::store_configured(
            std::path::Path::new("/etc/velnor-host/github-token"),
            secret,
        );
    }
    #[cfg(target_os = "macos")]
    if let Some((service, account)) = keychain_parts(reference) {
        return import_secret(service, account, secret);
    }
    let _ = reference;
    Err(HostError::Keychain)
}

/// Remove only the exact configured host credential after a completed local
/// disconnect. The remote runner group and Scale Set are never deleted here.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the configured host source is
/// unsupported or the exact credential cannot be safely removed.
pub fn remove_configured_secret(reference: &str) -> Result<(), HostError> {
    #[cfg(target_os = "linux")]
    if reference == "systemd-credential:github-token" {
        return linux::remove_configured(std::path::Path::new("/etc/velnor-host/github-token"));
    }
    #[cfg(target_os = "macos")]
    if is_configured_macos_credential_reference(reference) {
        return security_framework::passwords::delete_generic_password(
            MACOS_CREDENTIAL_SERVICE,
            MACOS_CREDENTIAL_ACCOUNT,
        )
        .map_err(|_| HostError::Keychain);
    }
    let _ = reference;
    Err(HostError::Keychain)
}

#[cfg(any(target_os = "macos", test))]
#[must_use]
fn is_configured_macos_credential_reference(reference: &str) -> bool {
    reference == MACOS_CREDENTIAL_REF
}

/// Load the configured credential from the selected host-only source.
/// Linux reads only systemd's `CREDENTIALS_DIRECTORY/github-token` file.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the configured host source is missing,
/// malformed, or fails owner and permission checks.
pub fn load_configured_secret(reference: &str) -> Result<Zeroizing<Vec<u8>>, HostError> {
    #[cfg(target_os = "linux")]
    if reference == "systemd-credential:github-token" {
        return linux::load_configured();
    }
    #[cfg(target_os = "macos")]
    if let Some((service, account)) = keychain_parts(reference) {
        return load_secret(service, account);
    }
    let _ = reference;
    Err(HostError::Keychain)
}

/// Load the dedicated read-only GitHub Actions REST credential.
///
/// Linux reads only systemd's `CREDENTIALS_DIRECTORY/actions-read-token`;
/// macOS reads only the fixed Velnor Keychain item. This role is separate
/// from the controller's configured registration credential.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the dedicated credential is absent,
/// malformed, or fails the selected host backend's ownership checks.
pub fn load_actions_read_token() -> Result<Zeroizing<Vec<u8>>, HostError> {
    #[cfg(target_os = "linux")]
    {
        return linux::load_actions_read_token();
    }
    #[cfg(target_os = "macos")]
    {
        return load_secret(
            MACOS_ACTIONS_READ_TOKEN_SERVICE,
            MACOS_ACTIONS_READ_TOKEN_ACCOUNT,
        );
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(HostError::Keychain)
    }
}

#[cfg(target_os = "macos")]
fn keychain_parts(reference: &str) -> Option<(&str, &str)> {
    let value = reference.strip_prefix("keychain:")?;
    let (service, account) = value.rsplit_once('/')?;
    if service.is_empty() || account.is_empty() || account.contains('/') {
        return None;
    }
    Some((service, account))
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
