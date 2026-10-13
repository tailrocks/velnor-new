//! Keychain import. Tests use only `com.tailrocks.velnor.host.test`.

use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

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
struct CliDelete {
    service: &'static str,
    account: &'static str,
}

#[cfg(target_os = "macos")]
impl Drop for CliDelete {
    fn drop(&mut self) {
        let removed = Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-a",
                self.account,
                "-s",
                self.service,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match removed {
            Ok(_) | Err(_) => {}
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn round_trip_missing_and_denied_keep_prompts_enabled() -> Result<(), HostError> {
    prompts_enabled()?;
    round_trip()?;
    prompts_enabled()?;
    missing_item()?;
    prompts_enabled()?;
    denied_acl()?;
    prompts_enabled()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn interaction_allowed() -> Result<bool, HostError> {
    security_framework::os::macos::keychain::SecKeychain::user_interaction_allowed()
        .map_err(|_| HostError::Keychain)
}

#[cfg(target_os = "macos")]
fn prompts_enabled() -> Result<(), HostError> {
    if interaction_allowed()? {
        Ok(())
    } else {
        Err(HostError::Keychain)
    }
}

#[cfg(target_os = "macos")]
fn round_trip() -> Result<(), HostError> {
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

#[cfg(target_os = "macos")]
fn missing_item() -> Result<(), HostError> {
    if load_secret("com.tailrocks.velnor.host.test.missing", "absent").is_ok() {
        return Err(HostError::Keychain);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn denied_acl() -> Result<(), HostError> {
    let service = "com.tailrocks.velnor.host.test.denied";
    let account = "velnor-host-denied";
    let _guard = TestItem { service, account };
    let _cli = CliDelete { service, account };
    let status = Command::new("/usr/bin/security")
        .args([
            "add-generic-password",
            "-U",
            "-a",
            account,
            "-s",
            service,
            "-w",
            "canary-token",
            "-T",
            "/usr/bin/false",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| HostError::Keychain)?;
    if !status.success() {
        return Err(HostError::Keychain);
    }
    let saw_disabled = AtomicBool::new(false);
    let copied = crate::keychain::copy_without_prompt(service, account, || {
        if interaction_allowed()? {
            return Err(HostError::Keychain);
        }
        saw_disabled.store(true, Ordering::Relaxed);
        Ok(())
    });
    // A missing guard fails here, before the production fetch can open SecurityAgent.
    if !saw_disabled.load(Ordering::Relaxed) || copied.is_ok() {
        return Err(HostError::Keychain);
    }
    if load_secret(service, account).is_ok() {
        return Err(HostError::Keychain);
    }
    Ok(())
}
