//! Stdin token stored with the macOS Security framework.
//!
//! Non-macOS imports fail. The token is not an argument, a log line, or an error payload.

use std::io::Read;
#[cfg(any(target_os = "macos", test))]
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::error::HostError;

const MAX_SECRET_LEN: usize = 4096;

#[cfg(any(target_os = "macos", test))]
static KEYCHAIN_INTERACTION: Mutex<()> = Mutex::new(());

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
    // An ACL mismatch must return. Launchd has no window for a prompt.
    copy_without_prompt(service, account, || Ok(()))
}

/// Copy the token for `service` and `account` while prompts are disabled.
///
/// `during` runs while that guard is still held.
///
/// # Errors
///
/// Returns [`HostError::Keychain`] when the framework or `during` fails.
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

#[cfg(any(target_os = "macos", test))]
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

#[cfg(test)]
mod interaction_tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::{HostError, with_keychain_copy};

    static INTERACTION_ALLOWED: AtomicBool = AtomicBool::new(true);

    struct PromptGuard;

    impl Drop for PromptGuard {
        fn drop(&mut self) {
            INTERACTION_ALLOWED.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn overlapping_copies_keep_the_process_prompt_flag_disabled() -> Result<(), HostError> {
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let first = thread::spawn(move || {
            with_keychain_copy(
                || Ok(INTERACTION_ALLOWED.load(Ordering::SeqCst)),
                || {
                    INTERACTION_ALLOWED.store(false, Ordering::SeqCst);
                    Ok(PromptGuard)
                },
                || {
                    first_started_tx.send(()).map_err(|_| HostError::Keychain)?;
                    release_first_rx
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| HostError::Keychain)?;
                    if INTERACTION_ALLOWED.load(Ordering::SeqCst) {
                        return Err(HostError::Keychain);
                    }
                    Ok(())
                },
            )
        });
        if first_started_rx
            .recv_timeout(Duration::from_secs(5))
            .is_err()
        {
            drop(release_first_tx);
            let first_result = first.join().map_err(|_| HostError::Keychain)?;
            return first_result.and(Err(HostError::Keychain));
        }
        let second = thread::spawn(move || {
            with_keychain_copy(
                || Ok(INTERACTION_ALLOWED.load(Ordering::SeqCst)),
                || {
                    INTERACTION_ALLOWED.store(false, Ordering::SeqCst);
                    Ok(PromptGuard)
                },
                || {
                    if INTERACTION_ALLOWED.load(Ordering::SeqCst) {
                        return Err(HostError::Keychain);
                    }
                    second_started_tx
                        .send(())
                        .map_err(|_| HostError::Keychain)?;
                    Ok(())
                },
            )
        });
        let second_entered_early = second_started_rx
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        let first_kept_interaction_disabled = !INTERACTION_ALLOWED.load(Ordering::SeqCst);
        release_first_tx.send(()).map_err(|_| HostError::Keychain)?;
        let first_result = first.join().map_err(|_| HostError::Keychain)?;
        let second_entered_after = second_started_rx
            .recv_timeout(Duration::from_secs(5))
            .is_ok();
        let second_result = second.join().map_err(|_| HostError::Keychain)?;
        if second_entered_early
            || !first_kept_interaction_disabled
            || !second_entered_after
            || first_result.is_err()
            || second_result.is_err()
            || !INTERACTION_ALLOWED.load(Ordering::SeqCst)
        {
            return Err(HostError::Keychain);
        }

        initially_disabled_state_is_preserved()?;
        Ok(())
    }

    fn initially_disabled_state_is_preserved() -> Result<(), HostError> {
        INTERACTION_ALLOWED.store(false, Ordering::SeqCst);
        let disabled = with_keychain_copy(
            || Ok(INTERACTION_ALLOWED.load(Ordering::SeqCst)),
            || {
                INTERACTION_ALLOWED.store(false, Ordering::SeqCst);
                Ok(PromptGuard)
            },
            || Ok(!INTERACTION_ALLOWED.load(Ordering::SeqCst)),
        )?;
        let preserved = !INTERACTION_ALLOWED.load(Ordering::SeqCst);
        INTERACTION_ALLOWED.store(true, Ordering::SeqCst);
        if !disabled || !preserved {
            return Err(HostError::Keychain);
        }
        Ok(())
    }
}
