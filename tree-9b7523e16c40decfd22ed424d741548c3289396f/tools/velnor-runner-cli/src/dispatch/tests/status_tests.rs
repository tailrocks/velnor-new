use super::super::{
    ConfigObservation, DependencyObservation, JournalObservation, credential_is_available,
    journal_file_observation, status_observation_with,
};
use super::disconnect_state_path;

const LINUX_STATUS_CONFIG: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
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
    "image_profile = \"ubuntu-26.04-amd64\"\n",
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
    assert_eq!(document["readiness"]["controller_runtime"], "unknown");
    assert_eq!(document["readiness"]["work_admission"], "not_proven");
    assert_eq!(
        document["readiness"]["work_admission_reasons"],
        serde_json::json!([
            "image_profile_admission_not_proven",
            "apparmor_policy_admission_not_proven",
            "durable_controller_observation_unavailable"
        ])
    );
    assert!(!observation.json().contains("token"));
}

#[test]
fn local_status_evidence_never_promotes_linux_work_admission() {
    let observation = status_observation_with(
        Ok(Some(LINUX_STATUS_CONFIG.to_owned())),
        velnor_runner_host::HostPlatform::Linux,
        JournalObservation::PresentUnverified,
        crate::service::ControllerServiceState::InUse,
        |_| Ok(true),
        |_| {
            Ok(velnor_runner_host::docker_client::DockerVersion {
                server_version: "29.8.2".to_owned(),
                api_version: Some("1.53".to_owned()),
                os: Some("linux".to_owned()),
                architecture: Some("amd64".to_owned()),
            })
        },
    );

    let document: serde_json::Value =
        serde_json::from_str(&observation.json()).expect("valid status JSON");
    assert_eq!(document["credential"], "available");
    assert_eq!(document["docker"], "available");
    assert_eq!(document["controller_service"], "in_use");
    assert_eq!(document["readiness"]["work_admission"], "not_proven");
}

#[test]
fn legacy_macos_status_shape_does_not_gain_linux_readiness_fields() {
    const MAC_CONFIG: &str = concat!(
        "schema = 1\n",
        "[github]\n",
        "repository = \"tailrocks/velnor-new\"\n",
        "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
        "credential_ref = \"keychain:com.tailrocks.velnor.host/velnor-host\"\n",
        "[host]\n",
        "[docker]\n",
        "context = \"orbstack\"\n",
        "platform = \"linux/amd64\"\n",
        "endpoint = \"unix:///var/run/docker.sock\"\n",
    );
    let observation = status_observation_with(
        Ok(Some(MAC_CONFIG.to_owned())),
        velnor_runner_host::HostPlatform::Macos,
        JournalObservation::Missing,
        crate::service::ControllerServiceState::Stopped,
        |_| Ok(true),
        |_| Err(velnor_runner_host::HostError::Docker),
    );
    let document: serde_json::Value =
        serde_json::from_str(&observation.json()).expect("valid status JSON");

    assert!(document.get("readiness").is_none());
    assert!(!observation.lines().contains("work_admission"));
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
        "state=not_proven\nconfig=invalid\ncredential=not_checked\ndocker=not_checked\njournal=missing\ncontroller_service=unknown\nglobal_readiness=not_proven\ncontroller_runtime=unknown\nwork_admission=not_proven\nwork_admission_reasons=image_profile_admission_not_proven,apparmor_policy_admission_not_proven,durable_controller_observation_unavailable"
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
