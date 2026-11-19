// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
#[cfg(target_os = "macos")]
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::error::HostError;

const MAX_SECRET_LEN: usize = 4096;

#[cfg(target_os = "macos")]
static KEYCHAIN_INTERACTION: Mutex<()> = Mutex::new(());

pub struct NormalizedSecret(Zeroizing<Vec<u8>>);

impl std::fmt::Debug for NormalizedSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NormalizedSecret([REDACTED])")
    }
}

pub trait SecretStore {
    fn store(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), HostError>;
}

struct SystemKeychain;

impl SecretStore for SystemKeychain {
    fn store(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
        store(service, account, secret)
    }
}

pub fn read_secret<R: Read>(reader: &mut R) -> Result<NormalizedSecret, HostError> {
    let mut secret = Zeroizing::new(Vec::new());
    reader
        .take(read_limit())
        .read_to_end(&mut secret)
        .map_err(|_| HostError::Keychain)?;
    if secret.len() > MAX_SECRET_LEN + 2 {
        return Err(HostError::Keychain);
    }
    let length = secret.len();
    if secret.ends_with(b"\r\n") {
        secret.truncate(length - 2);
    } else if secret.ends_with(b"\n") {
        secret.truncate(length - 1);
    }
    if secret.is_empty() {
        return Err(HostError::EmptySecret);
    }
    if secret.len() > MAX_SECRET_LEN || !secret.iter().all(u8::is_ascii_graphic) {
        return Err(HostError::Keychain);
    }
    Ok(NormalizedSecret(secret))
}

pub fn store_secret<S: SecretStore>(
    store: &S,
    service: &str,
    account: &str,
    secret: &NormalizedSecret,
) -> Result<(), HostError> {
    store.store(service, account, &secret.0)
}

pub fn import_secret(
    service: &str,
    account: &str,
    secret: &NormalizedSecret,
) -> Result<(), HostError> {
    store_secret(&SystemKeychain, service, account, secret)
}

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
    copy_without_prompt(service, account, || Ok(()))
}

#[cfg(target_os = "macos")]
pub(crate) fn copy_without_prompt<F>(
    service: &str,
    account: &str,
    during: F,
) -> Result<Vec<u8>, HostError>
where
    F: FnOnce() -> Result<(), HostError>,
{
    with_keychain_copy(
        || {
            security_framework::os::macos::keychain::SecKeychain::user_interaction_allowed()
                .map_err(|_| HostError::Keychain)
        },
        || {
            security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()
                .map_err(|_| HostError::Keychain)
        },
        || {
            during()?;
            security_framework::passwords::generic_password(
                security_framework::passwords::PasswordOptions::new_generic_password(
                    service, account,
                ),
            )
            .map_err(|_| HostError::Keychain)
        },
    )
}

#[cfg(target_os = "macos")]
fn with_keychain_copy<T, G, A, D, C>(
    interaction_allowed: A,
    disable_interaction: D,
    copy: C,
) -> Result<T, HostError>
where
    A: FnOnce() -> Result<bool, HostError>,
    D: FnOnce() -> Result<G, HostError>,
    C: FnOnce() -> Result<T, HostError>,
{
    let _serialized = KEYCHAIN_INTERACTION
        .lock()
        .map_err(|_| HostError::Keychain)?;
    if interaction_allowed()? {
        let _no_prompt = disable_interaction()?;
        copy()
    } else {
        copy()
    }
}

#[cfg(not(target_os = "macos"))]
fn fetch(service: &str, account: &str) -> Result<Vec<u8>, HostError> {
    let _kept = (service.len(), account.len());
    Err(HostError::Keychain)
}

fn read_limit() -> u64 {
    match u64::try_from(MAX_SECRET_LEN) {
        Ok(len) => len.saturating_add(3),
        Err(_) => 0,
    }
}
