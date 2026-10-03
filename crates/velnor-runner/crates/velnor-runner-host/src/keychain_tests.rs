//! Keychain import. Tests use only `com.tailrocks.velnor.host.test`.

use std::io::Cursor;

use crate::{HostError, read_secret};
#[cfg(target_os = "macos")]
use crate::{import_secret, load_secret};

#[test]
fn read_secret_keeps_the_canary_out_of_errors() -> Result<(), HostError> {
    let secret = read_secret(&mut Cursor::new(b"canary-token\n"))?;
    if secret.as_slice() != b"canary-token\n" {
        return Err(HostError::Keychain);
    }
    let Err(empty) = read_secret(&mut Cursor::new(b"")) else {
        return Err(HostError::EmptySecret);
    };
    if empty != HostError::EmptySecret || format!("{empty}").contains("canary-token") {
        return Err(HostError::Keychain);
    }
    let exact = read_secret(&mut Cursor::new(vec![b'a'; 4096]))?;
    if exact.len() != 4096 {
        return Err(HostError::Keychain);
    }
    let mut over = vec![0_u8; 4097];
    let marker = b"canary-token";
    over[..marker.len()].copy_from_slice(marker);
    let Err(big) = read_secret(&mut Cursor::new(over)) else {
        return Err(HostError::Keychain);
    };
    if big != HostError::Keychain || format!("{big}").contains("canary-token") {
        return Err(HostError::Keychain);
    }
    if format!("{}", HostError::Keychain).contains("canary-token") {
        return Err(HostError::Keychain);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
struct TestItem {
    service: &'static str,
    account: &'static str,
}

#[cfg(target_os = "macos")]
impl Drop for TestItem {
    fn drop(&mut self) {
        let removed =
            security_framework::passwords::delete_generic_password(self.service, self.account);
        match removed {
            Ok(()) | Err(_) => {}
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn import_secret_round_trips_the_test_service() -> Result<(), HostError> {
    let service = "com.tailrocks.velnor.host.test";
    let account = "velnor-host-test";
    let _guard = TestItem { service, account };
    let canary = b"canary-token";
    import_secret(service, account, canary)?;
    let loaded = load_secret(service, account)?;
    if loaded.as_slice() != canary || format!("{}", HostError::Keychain).contains("canary-token") {
        return Err(HostError::Keychain);
    }
    let stored = security_framework::passwords::generic_password(
        security_framework::passwords::PasswordOptions::new_generic_password(service, account),
    )
    .map_err(|_| HostError::Keychain)?;
    if stored.as_slice() != canary {
        return Err(HostError::Keychain);
    }
    Ok(())
}
