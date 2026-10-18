use super::{ReleaseReadSecret, token_text};
use crate::config_test_support::{basic_connection, host_config};
use crate::{HostConfig, HostError};

fn config(release_reader: Option<&str>) -> Result<HostConfig, HostError> {
    let connection = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let mut text = host_config(&[connection], 1);
    if let Some(reference) = release_reader {
        text = text.replace(
            "[host]\n",
            &format!("[release_read]\ncredential_ref = \"{reference}\"\n[host]\n"),
        );
    }
    HostConfig::parse(&text)
}

#[test]
fn missing_host_release_reference_fails_before_keychain_access() -> Result<(), HostError> {
    let config = config(None)?;
    assert_eq!(
        ReleaseReadSecret::load(&config).err(),
        Some(HostError::Keychain)
    );
    Ok(())
}

#[test]
fn repository_credential_cannot_be_used_for_release_reads() {
    let text = host_config(
        &[basic_connection(
            "essential",
            101,
            "donbeave/essential-mac",
            "essential",
            "essential-scale-set",
        )],
        1,
    )
    .replace(
        "[host]\n",
        "[release_read]\ncredential_ref = \"keychain:com.tailrocks.velnor.host/essential\"\n[host]\n",
    );
    assert_eq!(HostConfig::parse(&text).err(), Some(HostError::Config));
}

#[test]
fn release_token_accepts_one_stdin_line_ending_only() {
    assert_eq!(token_text(b"ghp_test-token\n"), Ok("ghp_test-token"));
    assert_eq!(token_text(b"ghp_test-token\r\n"), Ok("ghp_test-token"));
    assert_eq!(token_text(b"ghp_test-token"), Ok("ghp_test-token"));
    assert_eq!(token_text(b"\n"), Err(HostError::EmptySecret));
    assert_eq!(token_text(b"ghp_test-token\n\n"), Err(HostError::Keychain));
    assert_eq!(token_text(b"ghp_test-token "), Err(HostError::Keychain));
    assert_eq!(token_text(b"\xff"), Err(HostError::Keychain));
}
