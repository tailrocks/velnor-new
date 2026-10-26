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

pub fn import_secret(service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
    store(service, account, secret)
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
        Ok(len) => len.saturating_add(1),
        Err(_) => 0,
    }
}
