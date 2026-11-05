// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn gh_api_does_not_inherit_credentials_or_transport_overrides() {
    let parent = [
        ("PATH".into(), "/usr/bin".into()),
        ("HOME".into(), "/tmp/untrusted".into()),
        ("GH_TOKEN".into(), "dummy".into()),
        ("GITHUB_TOKEN".into(), "dummy".into()),
        ("HTTPS_PROXY".into(), "http://127.0.0.1:1234".into()),
        ("SSL_CERT_FILE".into(), "/tmp/fake-ca".into()),
    ];
    let child = crate::command::EnvPolicy::GhApi.child_env(&parent, &[]);
    assert!(
        child
            .iter()
            .any(|(key, value)| key == "HOME" && value == "/proc/self/fd")
    );
    assert!(
        child
            .iter()
            .any(|(key, value)| key == "GH_CONFIG_DIR" && value == "/proc/self/fd")
    );
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "HTTPS_PROXY", "SSL_CERT_FILE"] {
        assert!(!child.iter().any(|(name, _)| name == key), "{key}");
    }
}

#[test]
fn github_environment_token_selection_rejects_invalid_and_falls_back_when_empty()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::requests::select_environment_token;
    assert_eq!(
        select_environment_token(Some(""), Some("fallback-token"))?,
        Some("fallback-token")
    );
    assert_eq!(select_environment_token(None, None)?, None);
    assert_eq!(
        select_environment_token(Some("primary-token"), Some("fallback-token"))?,
        Some("primary-token")
    );
    assert!(select_environment_token(Some("bad token"), Some("fallback-token")).is_err());
    assert!(select_environment_token(Some(""), Some("bad token")).is_err());
    Ok(())
}

#[test]
fn github_commands_reject_explicit_transport_overrides() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsStr;
    let exec = crate::PinnedToolExec::new(
        vec![crate::PinnedTool::Gh],
        OsStr::new("gh"),
        vec!["api".into(), "user".into()],
    )?;
    let catalog = crate::ToolCatalog::pinned();
    for key in ["HTTPS_PROXY", "SSL_CERT_FILE", "SSL_CERT_DIR", "LD_PRELOAD"] {
        let overlay = [(key.into(), "untrusted".into())];
        assert!(exec.command(&catalog)?.with_env(&overlay).is_err());
        assert!(
            exec.command_with_github_token(&catalog, "dummy-token")?
                .with_env(&overlay)
                .is_err()
        );
    }
    Ok(())
}
