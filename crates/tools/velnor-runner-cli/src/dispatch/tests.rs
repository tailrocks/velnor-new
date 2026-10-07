//! Dispatch-level command tests.

mod connect_common;
mod connect_guard_tests;
mod connect_tests;
mod doctor_tests;

use std::path::Path;

use clap::Parser;

use super::{
    ConfigObservation, DependencyObservation, JournalObservation, config_path,
    credential_is_available, disconnect_for_os, journal_file_observation, requested_wait,
    selected_config_path, status_observation_with,
};
use crate::args::Cli;

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DISCONNECT_STATE: AtomicUsize = AtomicUsize::new(0);

fn disconnect_state_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-disconnect-{}-{}",
        std::process::id(),
        NEXT_DISCONNECT_STATE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn explicit_config_path_is_preserved() {
    let selected = config_path(
        Some(Path::new("/tmp/velnor-host-test.toml")),
        Path::new("/var/lib/velnor-host"),
    );
    assert_eq!(selected, Path::new("/tmp/velnor-host-test.toml"));
}

#[test]
fn daemon_uses_the_global_config_override() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "--config",
        "/etc/velnor-host/operator.toml",
        "daemon",
        "run",
    ])
    .map_err(|error| error.to_string())?;
    let state = Path::new("/var/lib/velnor-host");
    assert_eq!(
        selected_config_path(&cli, state),
        Path::new("/etc/velnor-host/operator.toml")
    );
    assert!(matches!(cli.command, crate::args::Command::Daemon { .. }));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_config_uses_the_package_owned_path() {
    assert_eq!(
        config_path(None, Path::new("/var/lib/velnor-host")),
        Path::new(velnor_runner_host::LINUX_CONFIG_PATH)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_default_config_stays_under_application_support() {
    assert_eq!(
        config_path(
            None,
            Path::new("/Users/example/Library/Application Support/Velnor")
        ),
        Path::new("/Users/example/Library/Application Support/Velnor/host.toml")
    );
}

#[test]
fn disconnect_requires_explicit_drain_and_wait() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, false, false, None, "macos"),
        ExitCode::from(2)
    );
    assert!(!state.exists());
}

#[test]
fn macos_disconnect_records_only_the_legacy_marker_and_never_claims_disconnect()
-> Result<(), String> {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, true, true, Some(30), "macos"),
        ExitCode::from(1)
    );
    let marker = std::fs::read(state.join("drain")).map_err(|error| error.to_string())?;
    assert_eq!(marker, b"1");
    assert_eq!(requested_wait(Some(30)), "the requested 30-second");
    assert_eq!(
        velnor_runner_host::disconnect_effects(velnor_runner_host::SetOwnership::Adopted, true),
        vec![velnor_runner_host::DisconnectEffect::Drain]
    );
    std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn linux_disconnect_wait_fails_without_remote_or_marker_effects() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, true, true, None, "linux"),
        ExitCode::from(1)
    );
    assert!(!state.exists());
    assert_eq!(requested_wait(None), "the configured-timeout");
}

const LINUX_STATUS_CONFIG: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-24.04-scale-set\"\n",
    "credential_ref = \"systemd-credential:github-token\"\n",
    "registration_scope = \"repository\"\n",
    "runner_group_id = 1\n",
    "runner_group_name = \"Default\"\n",
    "[host]\n",
    "platform = \"linux\"\n",
    "max_jobs = 1\n",
    "drain_timeout_secs = 900\n",
    "[trust]\n",
    "allowed_repositories = [\"ChainArgos/java-monorepo\"]\n",
    "allowed_events = [\"push\"]\n",
    "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\n",
    "allow_forks = false\n",
    "[runner]\n",
    "image_profile = \"ubuntu-24.04-amd64\"\n",
    "[docker]\n",
    "context = \"system\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn status_reports_real_local_observations_without_claiming_global_readiness() {
    let observation = status_observation_with(
        Ok(Some(LINUX_STATUS_CONFIG.to_owned())),
        velnor_runner_host::HostPlatform::Linux,
        JournalObservation::PresentUnverified,
        crate::service::ControllerServiceState::InUse,
        |reference| {
            assert_eq!(reference, "systemd-credential:github-token");
            Ok(true)
        },
        |endpoint| {
            assert_eq!(endpoint, "unix:///var/run/docker.sock");
            Ok(velnor_runner_host::docker_client::DockerVersion {
                server_version: "29.8.2".to_owned(),
                api_version: Some("1.53".to_owned()),
                os: Some("linux".to_owned()),
                architecture: Some("amd64".to_owned()),
            })
        },
    );

    assert_eq!(observation.config, ConfigObservation::Valid);
    assert_eq!(observation.credential, DependencyObservation::Available);
    assert_eq!(observation.docker, DependencyObservation::Available);
    let document: serde_json::Value =
        serde_json::from_str(&observation.json()).expect("valid status JSON");
    assert_eq!(document["journal"], "present_unverified");
    assert_eq!(document["controller_service"], "in_use");
    assert_eq!(document["global_readiness"], "not_proven");
    assert!(!observation.json().contains("token"));
}

#[test]
fn status_does_not_probe_credentials_or_docker_without_valid_config() {
    let credential_probed = std::cell::Cell::new(false);
    let docker_probed = std::cell::Cell::new(false);
    let observation = status_observation_with(
        Ok(Some("schema = 99\n".to_owned())),
        velnor_runner_host::HostPlatform::Linux,
        JournalObservation::Missing,
        crate::service::ControllerServiceState::Unknown,
        |_| {
            credential_probed.set(true);
            Ok(true)
        },
        |_| {
            docker_probed.set(true);
            Err(velnor_runner_host::HostError::Docker)
        },
    );

    assert_eq!(observation.config, ConfigObservation::Invalid);
    assert_eq!(observation.credential, DependencyObservation::NotChecked);
    assert_eq!(observation.docker, DependencyObservation::NotChecked);
    assert!(!credential_probed.get());
    assert!(!docker_probed.get());
    assert_eq!(
        observation.lines(),
        "state=not_proven\nconfig=invalid\ncredential=not_checked\ndocker=not_checked\njournal=missing\ncontroller_service=unknown\nglobal_readiness=not_proven"
    );
}

#[test]
fn status_keeps_credential_and_docker_failures_distinct() {
    let observation = status_observation_with(
        Ok(Some(LINUX_STATUS_CONFIG.to_owned())),
        velnor_runner_host::HostPlatform::Linux,
        JournalObservation::Unknown,
        crate::service::ControllerServiceState::Stopped,
        |_| Err(velnor_runner_host::HostError::Keychain),
        |_| Err(velnor_runner_host::HostError::Docker),
    );

    assert_eq!(observation.config, ConfigObservation::Valid);
    assert_eq!(observation.credential, DependencyObservation::Unavailable);
    assert_eq!(observation.docker, DependencyObservation::Unavailable);
    assert!(
        observation
            .json()
            .contains("\"controller_service\":\"stopped_or_absent\"")
    );
    assert!(
        observation
            .json()
            .contains("\"global_readiness\":\"not_proven\"")
    );
}

#[test]
fn credential_status_checks_nonempty_utf8_without_rendering_the_secret() {
    assert!(credential_is_available(b"ghp_private_value\n"));
    assert!(!credential_is_available(b" \t\n"));
    assert!(!credential_is_available(&[0xff, 0xfe]));
}

#[cfg(unix)]
#[test]
fn journal_status_never_calls_a_symlink_a_verified_journal() -> Result<(), String> {
    use std::os::unix::fs::symlink;

    let state = disconnect_state_path();
    std::fs::create_dir_all(&state).map_err(|error| error.to_string())?;
    let target = state.join("target");
    std::fs::write(&target, b"not a journal").map_err(|error| error.to_string())?;
    symlink(&target, state.join("launch.db")).map_err(|error| error.to_string())?;

    assert_eq!(journal_file_observation(&state), JournalObservation::Unsafe);
    std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}
