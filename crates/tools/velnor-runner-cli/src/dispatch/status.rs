use std::path::Path;
use std::process::ExitCode;

use velnor_runner_host::docker_client::{self, DockerVersion};
use velnor_runner_host::{
    HostConfig, HostError, HostPlatform, Readiness, doctor_json, load_configured_secret,
    read_host_config_file, readiness_for_empty,
};

pub(super) fn print_status(state: &Path, config_path: &Path, json: bool) -> ExitCode {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let platform = host_platform();
        let observation = status_observation_with(
            read_host_config_file(config_path, platform),
            platform,
            journal_file_observation(state),
            crate::service::controller_service_state(),
            |reference| {
                load_configured_secret(reference)
                    .map(|secret| credential_is_available(secret.as_slice()))
            },
            docker_client::read_version_blocking,
        );
        if json {
            println!("{}", observation.json());
        } else {
            println!("{}", observation.lines());
        }
        ExitCode::SUCCESS
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (state, config_path);
        let observation = StatusObservation {
            config: ConfigObservation::Unavailable,
            credential: DependencyObservation::NotChecked,
            docker: DependencyObservation::NotChecked,
            journal: JournalObservation::Unknown,
            controller_service: crate::service::ControllerServiceState::Unknown,
        };
        if json {
            println!("{}", observation.json());
        } else {
            println!("{}", observation.lines());
        }
        ExitCode::SUCCESS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigObservation {
    Valid,
    Missing,
    Invalid,
    Unavailable,
}

impl ConfigObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DependencyObservation {
    Available,
    Unavailable,
    NotChecked,
}

impl DependencyObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::NotChecked => "not_checked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JournalObservation {
    PresentUnverified,
    Missing,
    Unsafe,
    Unknown,
}

impl JournalObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PresentUnverified => "present_unverified",
            Self::Missing => "missing",
            Self::Unsafe => "unsafe",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StatusObservation {
    pub(super) config: ConfigObservation,
    pub(super) credential: DependencyObservation,
    pub(super) docker: DependencyObservation,
    pub(super) journal: JournalObservation,
    pub(super) controller_service: crate::service::ControllerServiceState,
}

impl StatusObservation {
    pub(super) fn json(self) -> String {
        serde_json::json!({
            "command": "status",
            "state": "not_proven",
            "config": self.config.as_str(),
            "credential": self.credential.as_str(),
            "docker": self.docker.as_str(),
            "journal": self.journal.as_str(),
            "controller_service": controller_service_status(self.controller_service),
            "global_readiness": "not_proven",
        })
        .to_string()
    }

    pub(super) fn lines(self) -> String {
        format!(
            "state=not_proven\nconfig={}\ncredential={}\ndocker={}\njournal={}\ncontroller_service={}\nglobal_readiness=not_proven",
            self.config.as_str(),
            self.credential.as_str(),
            self.docker.as_str(),
            self.journal.as_str(),
            controller_service_status(self.controller_service),
        )
    }
}

fn controller_service_status(state: crate::service::ControllerServiceState) -> &'static str {
    match state {
        crate::service::ControllerServiceState::InUse => "in_use",
        crate::service::ControllerServiceState::Stopped => "stopped_or_absent",
        crate::service::ControllerServiceState::Unknown => "unknown",
    }
}

pub(super) fn status_observation_with(
    config_text: Result<Option<String>, HostError>,
    platform: HostPlatform,
    journal: JournalObservation,
    controller_service: crate::service::ControllerServiceState,
    credential_probe: impl FnOnce(&str) -> Result<bool, HostError>,
    docker_probe: impl FnOnce(&str) -> Result<DockerVersion, HostError>,
) -> StatusObservation {
    let mut observation = StatusObservation {
        config: ConfigObservation::Unavailable,
        credential: DependencyObservation::NotChecked,
        docker: DependencyObservation::NotChecked,
        journal,
        controller_service,
    };
    let text = match config_text {
        Ok(Some(text)) => text,
        Ok(None) => {
            observation.config = ConfigObservation::Missing;
            return observation;
        }
        Err(_) => return observation,
    };
    let Ok(config) = HostConfig::parse(&text) else {
        observation.config = ConfigObservation::Invalid;
        return observation;
    };
    if config.validate_for_host(platform).is_err() {
        observation.config = ConfigObservation::Invalid;
        return observation;
    }
    observation.config = ConfigObservation::Valid;
    observation.credential = match credential_probe(&config.github.credential_ref) {
        Ok(true) => DependencyObservation::Available,
        Ok(false) | Err(_) => DependencyObservation::Unavailable,
    };
    observation.docker = match docker_probe(&config.docker.endpoint) {
        Ok(_) => DependencyObservation::Available,
        Err(_) => DependencyObservation::Unavailable,
    };
    observation
}

pub(super) fn credential_is_available(secret: &[u8]) -> bool {
    std::str::from_utf8(secret).is_ok_and(|value| !value.trim().is_empty())
}

pub(super) fn journal_file_observation(state: &Path) -> JournalObservation {
    match std::fs::symlink_metadata(state.join("launch.db")) {
        Ok(metadata) if metadata.file_type().is_file() => JournalObservation::PresentUnverified,
        Ok(_) => JournalObservation::Unsafe,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => JournalObservation::Missing,
        Err(_) => JournalObservation::Unknown,
    }
}

pub(super) fn print_doctor(state: &Path, config_path: &Path, probe: bool) -> ExitCode {
    if !probe {
        println!("{}", doctor_json(observe(state), false));
        return ExitCode::SUCCESS;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let result = {
        let platform = host_platform();
        read_host_config_file(config_path, platform)
            .map_err(|_| DoctorProbeFailure::Config)
            .and_then(|config| config.ok_or(DoctorProbeFailure::Config))
            .and_then(|text| {
                probe_config_text(&text, platform, docker_client::read_version_blocking)
            })
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result = Err(DoctorProbeFailure::Config);
    println!("{}", doctor_probe_document(result.clone()));
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(target_os = "linux")]
pub(super) const fn host_platform() -> HostPlatform {
    HostPlatform::Linux
}

#[cfg(target_os = "macos")]
pub(super) const fn host_platform() -> HostPlatform {
    HostPlatform::Macos
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DoctorProbeFailure {
    Config,
    Docker,
}

pub(super) fn probe_config_text(
    text: &str,
    platform: HostPlatform,
    probe: impl FnOnce(&str) -> Result<DockerVersion, HostError>,
) -> Result<DockerVersion, DoctorProbeFailure> {
    let config = HostConfig::parse(text).map_err(|_| DoctorProbeFailure::Config)?;
    config
        .validate_for_host(platform)
        .map_err(|_| DoctorProbeFailure::Config)?;
    probe(&config.docker.endpoint).map_err(|_| DoctorProbeFailure::Docker)
}

pub(super) fn doctor_probe_document(
    result: Result<DockerVersion, DoctorProbeFailure>,
) -> serde_json::Value {
    match result {
        Ok(version) => serde_json::json!({
            "command": "doctor",
            "docker": {
                "status": "available",
                "server_version": version.server_version,
                "api_version": version.api_version,
                "os": version.os,
                "architecture": version.architecture,
            },
            "global_readiness": "not_proven",
        }),
        Err(failure) => serde_json::json!({
            "command": "doctor",
            "docker": { "status": "unavailable" },
            "failure": match failure {
                DoctorProbeFailure::Config => "invalid_or_unavailable_config",
                DoctorProbeFailure::Docker => "docker_version_probe_failed",
            },
            "global_readiness": "not_proven",
        }),
    }
}

pub(super) fn observe(_state: &Path) -> Readiness {
    readiness_for_empty()
}
