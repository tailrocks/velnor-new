use super::super::status::{
    ConfigObservation, DependencyObservation, JournalObservation, doctor_local_document,
    doctor_local_observation_with,
};
use crate::args::Cli;
use clap::Parser;

const LINUX_PROBE_CONFIG: &str = concat!(
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

const LEGACY_MAC_PROBE_CONFIG: &str = concat!(
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

#[test]
fn doctor_without_probe_reports_local_observations_without_claiming_readiness() {
    use crate::service::ControllerServiceState;
    use velnor_runner_host::HostPlatform;

    let observation = doctor_local_observation_with(
        Ok(Some(LINUX_PROBE_CONFIG.to_owned())),
        HostPlatform::Linux,
        JournalObservation::PresentUnverified,
        ControllerServiceState::InUse,
        |reference| {
            assert_eq!(reference, "systemd-credential:github-token");
            Ok(true)
        },
    );
    let document = doctor_local_document(observation);

    assert_eq!(observation.config, ConfigObservation::Valid);
    assert_eq!(observation.credential, DependencyObservation::Available);
    assert_eq!(observation.docker, DependencyObservation::NotChecked);
    assert_eq!(document["command"], "doctor");
    assert!(!document["probe"].as_bool().expect("boolean probe flag"));
    assert_eq!(document["journal"], "present_unverified");
    assert_eq!(document["controller_service"], "in_use");
    assert_eq!(document["global_readiness"], "not_proven");
    assert!(!document.to_string().contains("github-token"));
}

#[test]
fn doctor_without_probe_rejects_invalid_config_before_credential_access() {
    use crate::service::ControllerServiceState;
    use velnor_runner_host::HostPlatform;

    let credential_probed = std::cell::Cell::new(false);
    let observation = doctor_local_observation_with(
        Ok(Some("schema = 99\n".to_owned())),
        HostPlatform::Linux,
        JournalObservation::Missing,
        ControllerServiceState::Unknown,
        |_| {
            credential_probed.set(true);
            Ok(true)
        },
    );

    assert_eq!(observation.config, ConfigObservation::Invalid);
    assert_eq!(observation.credential, DependencyObservation::NotChecked);
    assert_eq!(observation.docker, DependencyObservation::NotChecked);
    assert!(!credential_probed.get());
    let document = doctor_local_document(observation);
    assert_eq!(document["global_readiness"], "not_proven");
}

#[test]
fn doctor_without_probe_preserves_legacy_macos_config_validation() {
    use crate::service::ControllerServiceState;
    use velnor_runner_host::HostPlatform;

    let observation = doctor_local_observation_with(
        Ok(Some(LEGACY_MAC_PROBE_CONFIG.to_owned())),
        HostPlatform::Macos,
        JournalObservation::Missing,
        ControllerServiceState::Stopped,
        |reference| {
            assert_eq!(reference, "keychain:com.tailrocks.velnor.host/velnor-host");
            Ok(true)
        },
    );

    assert_eq!(observation.config, ConfigObservation::Valid);
    assert_eq!(observation.credential, DependencyObservation::Available);
    assert_eq!(observation.docker, DependencyObservation::NotChecked);
}

#[test]
fn doctor_probe_uses_the_validated_configured_endpoint_and_never_claims_global_ready() {
    use super::super::{doctor_probe_document, probe_config_text};
    use velnor_runner_host::HostPlatform;
    use velnor_runner_host::docker_client::DockerVersion;

    let mut observed_endpoint = None;
    let result = probe_config_text(LINUX_PROBE_CONFIG, HostPlatform::Linux, |endpoint| {
        observed_endpoint = Some(endpoint.to_owned());
        Ok(DockerVersion {
            server_version: "29.8.2".to_owned(),
            api_version: Some("1.53".to_owned()),
            os: Some("linux".to_owned()),
            architecture: Some("amd64".to_owned()),
        })
    });

    assert_eq!(
        observed_endpoint.as_deref(),
        Some("unix:///var/run/docker.sock")
    );
    let document = doctor_probe_document(result);
    assert_eq!(document["docker"]["status"], "available");
    assert_eq!(document["docker"]["server_version"], "29.8.2");
    assert_eq!(document["global_readiness"], "not_proven");
}

#[test]
fn doctor_probe_rejects_invalid_config_before_contacting_docker() {
    use super::super::{DoctorProbeFailure, doctor_probe_document, probe_config_text};
    use velnor_runner_host::HostPlatform;
    use velnor_runner_host::docker_client::DockerVersion;

    let mut called = false;
    let result = probe_config_text(
        &LINUX_PROBE_CONFIG.replace("allow_forks = false", "allow_forks = true"),
        HostPlatform::Linux,
        |_| {
            called = true;
            Ok(DockerVersion {
                server_version: "unexpected".to_owned(),
                api_version: None,
                os: None,
                architecture: None,
            })
        },
    );

    assert_eq!(result, Err(DoctorProbeFailure::Config));
    assert!(!called);
    let document = doctor_probe_document(result);
    assert_eq!(document["docker"]["status"], "unavailable");
    assert_eq!(document["global_readiness"], "not_proven");
}

#[test]
fn doctor_probe_reports_docker_errors_without_exposing_endpoint_or_credentials() {
    use super::super::{DoctorProbeFailure, doctor_probe_document, probe_config_text};
    use velnor_runner_host::{HostError, HostPlatform};

    let result = probe_config_text(LINUX_PROBE_CONFIG, HostPlatform::Linux, |_| {
        Err::<velnor_runner_host::docker_client::DockerVersion, _>(HostError::Docker)
    });
    assert_eq!(result, Err(DoctorProbeFailure::Docker));

    let output = doctor_probe_document(result).to_string();
    assert!(output.contains("docker_version_probe_failed"));
    assert!(output.contains("not_proven"));
    assert!(!output.contains("/var/run/docker.sock"));
    assert!(!output.contains("github-token"));
}

#[test]
fn doctor_probe_accepts_existing_legacy_macos_config_shape() {
    use super::super::probe_config_text;
    use velnor_runner_host::HostPlatform;
    use velnor_runner_host::docker_client::DockerVersion;

    let mut called = false;
    let result = probe_config_text(LEGACY_MAC_PROBE_CONFIG, HostPlatform::Macos, |endpoint| {
        called = true;
        assert_eq!(endpoint, "unix:///var/run/docker.sock");
        Ok(DockerVersion {
            server_version: "27.0.0".to_owned(),
            api_version: None,
            os: Some("linux".to_owned()),
            architecture: Some("amd64".to_owned()),
        })
    });

    assert!(called);
    assert!(result.is_ok());
}

#[test]
fn doctor_probe_help_describes_only_the_read_only_engine_version_check() -> Result<(), String> {
    let Err(error) = Cli::try_parse_from(["velnor-host", "doctor", "--help"]) else {
        return Err("doctor help unexpectedly parsed as a command".to_owned());
    };
    let help = error.to_string();
    assert!(help.contains("GET /version"));
    assert!(help.contains("does not prove controller readiness"));
    assert!(!help.contains("Reconcile existing controller state"));
    Ok(())
}
