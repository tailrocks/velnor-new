use std::cell::Cell;
use std::io::Cursor;

use clap::CommandFactory;

use super::super::connect::{
    ConnectError, ConnectRequest, connect_with, host_platform_for, map_discovery_error,
    sample_config_for,
};
use super::connect_common::{
    TempDir, allowed_events, allowed_workflow_paths, expected_credential_ref, file_ops, request,
};
use crate::args::Cli;
use velnor_runner_host::{EnsureError, HostConfig, HostPlatform};

#[test]
fn connect_platform_selector_fails_closed_for_unsupported_targets() {
    assert_eq!(host_platform_for("linux"), Some(HostPlatform::Linux));
    assert_eq!(host_platform_for("macos"), Some(HostPlatform::Macos));
    assert_eq!(host_platform_for("freebsd"), None);
    assert_eq!(host_platform_for("windows"), None);
}

#[test]
fn connect_help_reads_stdin_and_has_no_token_flag() -> Result<(), String> {
    let mut command = Cli::command();
    let connect = command
        .find_subcommand_mut("connect")
        .ok_or("missing connect")?;
    let mut buffer = Vec::new();
    connect
        .write_long_help(&mut buffer)
        .map_err(|err| err.to_string())?;
    let text = String::from_utf8(buffer).map_err(|err| err.to_string())?;
    if !text.contains("stdin") || text.contains("--token") {
        return Err("connect help must identify stdin and omit token flags".to_owned());
    }
    Ok(())
}

#[test]
fn empty_stdin_does_not_create_or_overwrite_config() -> Result<(), String> {
    let dir = TempDir::new("empty")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = request(&path, &events, &workflow_paths);
    let mut empty = Cursor::new(b"");
    if connect_with(
        &mut empty,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_, _| Ok(()),
    )
    .is_ok()
    {
        return Err("empty stdin succeeded".to_owned());
    }
    if path.exists() {
        return Err("empty input created config".to_owned());
    }
    std::fs::write(&path, b"keep-me\n").map_err(|err| err.to_string())?;
    let mut again = Cursor::new(b"");
    if connect_with(
        &mut again,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_, _| Ok(()),
    )
    .is_ok()
    {
        return Err("empty overwrite succeeded".to_owned());
    }
    if std::fs::read(&path).map_err(|err| err.to_string())? != b"keep-me\n" {
        return Err("empty input overwrote config".to_owned());
    }
    Ok(())
}

#[test]
fn connect_validates_before_store_and_persists_secret_free_config() -> Result<(), String> {
    let dir = TempDir::new("connect")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = request(&path, &events, &workflow_paths);
    let canary = b"canary-token\n";
    let mut input = Cursor::new(&canary[..]);
    let validated = Cell::new(false);
    let stored = Cell::new(false);
    connect_with(
        &mut input,
        &request,
        file_ops(),
        |token, config| {
            if token != "canary-token" || config.github.repository != "example/repo" {
                return Err(super::super::connect::ConnectError::Rejected);
            }
            validated.set(true);
            Ok(())
        },
        |config, secret| {
            if !validated.get() || config.github.credential_ref != expected_credential_ref() {
                return Err(super::super::connect::ConnectError::Rejected);
            }
            stored.set(secret == canary);
            Ok(())
        },
    )
    .map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let parsed = HostConfig::parse(&text).map_err(|err| err.to_string())?;
    let trust = parsed.job_trust_policy().map_err(|err| err.to_string())?;
    if !validated.get()
        || !stored.get()
        || text.contains("canary-token")
        || !text.contains("managed_by = \"velnor-host-connect-v1\"")
        || trust.allowed_repositories != ["example/repo".to_owned()]
        || trust.allowed_events != allowed_events()
        || trust.allowed_workflow_paths != [".github/workflows/ci.yml".to_owned()]
    {
        return Err("connect did not safely validate and persist".to_owned());
    }
    #[cfg(target_os = "linux")]
    if trust.allowed_head_branches != ["main".to_owned()]
        || trust.allowed_group_workflows
            != ["example/repo/.github/workflows/ci.yml@refs/heads/main".to_owned()]
        || trust.workflow_rules.len() != 1
        || trust.workflow_rules[0].workflow_ref
            != "example/repo/.github/workflows/ci.yml@refs/heads/main"
        || trust.workflow_rules[0].job_workflow_ref
            != "example/repo/.github/workflows/ci.yml@refs/heads/main"
        || trust.workflow_rules[0].workflow_path != ".github/workflows/ci.yml@refs/heads/main"
        || trust.workflow_rules[0].event != "push"
        || trust.workflow_rules[0].head_branch != "main"
        || !trust.workflow_rules[0].referenced_workflows.is_empty()
    {
        return Err("Linux connect lost exact policy fields during persistence".to_owned());
    }
    #[cfg(target_os = "macos")]
    if !trust.allowed_head_branches.is_empty()
        || !trust.workflow_rules.is_empty()
        || !trust.allowed_group_workflows.is_empty()
    {
        return Err("legacy macOS policy acquired Linux-only fields".to_owned());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .map_err(|err| err.to_string())?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o640 {
            return Err("Linux config mode does not match the service contract".to_owned());
        }
    }
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn linux_trust_input_must_be_explicit_exact_and_rejected_before_stdin_or_writes()
-> Result<(), String> {
    let dir = TempDir::new("trust-policy-input")?;
    let config_path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let mut request = request(&config_path, &events, &workflow_paths);
    let trust_path = request
        .trust_policy_file
        .clone()
        .ok_or("Linux fixture omitted trust-policy path")?;
    let original = std::fs::read_to_string(&trust_path).map_err(|err| err.to_string())?;
    let invalid_policies = [
        original.replace("example/repo", "other/repo"),
        original.replace("\"push\", \"pull_request\"", "\"push\""),
        original.replace(
            "\".github/workflows/ci.yml\"",
            "\".github/workflows/other.yml\"",
        ),
        original.replace("\"allowed_head_branches\": [\"main\"]", "\"allowed_head_branches\": []"),
        original.replace("\"workflow_rules\": [{", "\"workflow_rules\": []"),
        original.replace(
            "\"allowed_group_workflows\": [\n                    \"example/repo/.github/workflows/ci.yml@refs/heads/main\"\n                ]",
            "\"allowed_group_workflows\": []",
        ),
        original.replace("\"allow_forks\": false", "\"allow_forks\": true"),
        format!("{}{}", original, " ".repeat(64 * 1024 + 1)),
        "{ malformed json".to_owned(),
    ];
    for invalid_policy in invalid_policies {
        std::fs::write(&trust_path, invalid_policy).map_err(|err| err.to_string())?;
        let mut input = Cursor::new(b"must-not-be-consumed\n");
        let remote_called = Cell::new(false);
        let stored = Cell::new(false);
        let result = connect_with(
            &mut input,
            &request,
            file_ops(),
            |_, _| {
                remote_called.set(true);
                Ok(())
            },
            |_, _| {
                stored.set(true);
                Ok(())
            },
        );
        if result.is_ok()
            || input.position() != 0
            || remote_called.get()
            || stored.get()
            || config_path.exists()
        {
            return Err(
                "invalid Linux trust input reached stdin, remote validation, or writes".to_owned(),
            );
        }
    }

    std::fs::write(&trust_path, &original).map_err(|err| err.to_string())?;
    request.trust_policy_file = None;
    let mut input = Cursor::new(b"must-not-be-consumed\n");
    let result = connect_with(
        &mut input,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_, _| Ok(()),
    );
    if result != Err(ConnectError::TrustPolicy) || input.position() != 0 || config_path.exists() {
        return Err("missing Linux trust input was not rejected before credential use".to_owned());
    }
    Ok(())
}

#[test]
fn macos_explicit_group_trust_options_are_preserved() -> Result<(), String> {
    let dir = TempDir::new("macos-explicit-trust")?;
    let config_path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = ConnectRequest {
        config_path: &config_path,
        repo: "example/repo",
        scale_set: "ubuntu-26.04-scale-set",
        platform: "linux/amd64",
        host_platform: Some("macos"),
        registration_scope: Some("repository"),
        runner_group_id: Some(7),
        runner_group_name: Some("Mac runners"),
        allowed_events: &events,
        allowed_workflow_paths: &workflow_paths,
        trust_policy_file: None,
        image_profile: None,
        max_jobs: None,
        drain_timeout_secs: None,
        docker_context: None,
        endpoint: None,
    };
    let text = sample_config_for(&request, HostPlatform::Macos)
        .map_err(|error| format!("render failed: {error}"))?;
    let expected_trust = "[trust]\nallowed_repositories = [\"example/repo\"]\nallowed_events = [\"push\", \"pull_request\"]\nallowed_workflow_paths = [\".github/workflows/ci.yml\"]\nallow_forks = false\n";
    if !text.contains(expected_trust) {
        return Err("explicit macOS trust block changed from the previous renderer".to_owned());
    }
    let config = HostConfig::parse(&text).map_err(|error| format!("parse failed: {error}"))?;
    config
        .validate_for_host(HostPlatform::Macos)
        .map_err(|error| format!("macOS validation failed: {error}"))?;
    let trust = config
        .job_trust_policy()
        .map_err(|_| "explicit macOS trust settings were omitted")?;
    if trust.allowed_repositories != ["example/repo".to_owned()]
        || trust.allowed_events != events
        || trust.allowed_workflow_paths != workflow_paths
        || trust.allow_forks
    {
        return Err("explicit macOS trust settings changed during rendering".to_owned());
    }
    Ok(())
}

#[test]
fn unavailable_group_policy_is_specific_and_does_not_save_config_or_secret() -> Result<(), String> {
    let policy_error = map_discovery_error(EnsureError::GroupPolicyUnavailable);
    if policy_error != ConnectError::GroupPolicyUnavailable
        || !policy_error
            .to_string()
            .contains("runner-group workflow policy could not be verified")
        || !policy_error
            .to_string()
            .contains("no new configuration or credential was stored")
    {
        return Err("missing group-policy evidence did not produce its safe diagnostic".to_owned());
    }
    if map_discovery_error(EnsureError::NotFound) != ConnectError::Rejected {
        return Err("ordinary discovery failures stopped using the generic rejection".to_owned());
    }

    let dir = TempDir::new("group-policy-unavailable")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = request(&path, &events, &workflow_paths);
    let mut input = Cursor::new(b"canary-token\n");
    let stored = Cell::new(false);
    let result = connect_with(
        &mut input,
        &request,
        file_ops(),
        |_token, _config| Err(policy_error),
        |_config, _secret| {
            stored.set(true);
            Ok(())
        },
    );
    if result != Err(policy_error) || path.exists() || stored.get() {
        return Err("unverified group policy reached config or credential persistence".to_owned());
    }
    Ok(())
}

#[test]
fn original_macos_connect_options_still_generate_the_legacy_binding() -> Result<(), String> {
    let dir = TempDir::new("legacy-macos")?;
    let events = Vec::new();
    let config_path = dir.path().join("host.toml");
    let mut request = ConnectRequest {
        config_path: &config_path,
        repo: "tailrocks/velnor-new",
        scale_set: "ubuntu-26.04-scale-set",
        platform: "linux/amd64",
        host_platform: None,
        registration_scope: None,
        runner_group_id: None,
        runner_group_name: None,
        allowed_events: &events,
        allowed_workflow_paths: &[],
        trust_policy_file: None,
        image_profile: None,
        max_jobs: None,
        drain_timeout_secs: None,
        docker_context: None,
        endpoint: None,
    };
    let text = sample_config_for(&request, HostPlatform::Macos).map_err(|e| e.to_string())?;
    let config = HostConfig::parse(&text).map_err(|e| e.to_string())?;
    config
        .validate_for_host(HostPlatform::Macos)
        .map_err(|e| e.to_string())?;
    if config.host.platform.is_some()
        || config.github.runner_group_id.is_some()
        || config.trust.is_some()
        || config.host.drain_timeout_secs.is_some()
        || config.github.credential_ref != "keychain:com.tailrocks.velnor.host/velnor-host"
    {
        return Err("the original macOS binding surface changed".to_owned());
    }
    request.trust_policy_file = Some(dir.path().join("trust-policy.json"));
    if sample_config_for(&request, HostPlatform::Macos) != Err(ConnectError::TrustPolicy) {
        return Err("the Linux-only trust input changed macOS connect behavior".to_owned());
    }
    Ok(())
}
