//! One formatter for the names owned by a physical runner reservation.

use crate::error::HostError;

/// One fixed physical volume in the worker ownership inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeRole {
    /// Runner socket directory mounted at `/run`.
    Socket,
    /// Initialized Runner home shared with `DinD` at `/home/runner`.
    Home,
    /// Actions runner workspace mounted at `_work`.
    Work,
    /// Private Docker-in-Docker data mounted at `/var/lib/docker`.
    DindData,
    /// Controller-owned volume for bounded runner identity and job facts.
    RunnerMetadata,
}

impl WorkerVolumeRole {
    pub(crate) const ALL: [Self; 5] = [
        Self::Socket,
        Self::Home,
        Self::Work,
        Self::DindData,
        Self::RunnerMetadata,
    ];

    #[cfg(test)]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Socket => "socket",
            Self::Home => "home",
            Self::Work => "work",
            Self::DindData => "dind-data",
            Self::RunnerMetadata => "engine-metadata",
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Socket => "",
            Self::Home => "-home",
            Self::Work => "-work",
            Self::DindData => "-docker",
            Self::RunnerMetadata => "-metadata",
        }
    }
}

/// The names derived from one physical reservation nonce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkerIdentityNames {
    private_volume: String,
    reservation_nonce: Option<String>,
}

impl WorkerIdentityNames {
    /// Derive all worker names from the reservation's lowercase 128-bit nonce.
    pub(crate) fn from_nonce(nonce: &str) -> Result<Self, HostError> {
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            private_volume: format!("w{nonce}"),
            reservation_nonce: Some(nonce.to_owned()),
        })
    }

    /// Use the recorded private-volume base for cleanup and inspection.
    pub(crate) fn from_private_volume(private_volume: &str) -> Result<Self, HostError> {
        if !valid_private_volume(private_volume) {
            return Err(HostError::ForbiddenMount);
        }
        let reservation_nonce = private_volume
            .strip_prefix('w')
            .filter(|nonce| {
                nonce.len() == 32
                    && nonce
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .map(str::to_owned);
        Ok(Self {
            private_volume: private_volume.to_owned(),
            reservation_nonce,
        })
    }

    pub(crate) fn private_volume(&self) -> &str {
        &self.private_volume
    }

    pub(crate) fn volume_name(&self, role: WorkerVolumeRole) -> String {
        format!("{}{suffix}", self.private_volume, suffix = role.suffix())
    }

    pub(crate) fn container_name(&self, role: WorkerContainerRole) -> String {
        match (self.reservation_nonce.as_deref(), role) {
            (Some(nonce), WorkerContainerRole::Runner) => format!("w{nonce}-runner"),
            (Some(nonce), WorkerContainerRole::Dind) => format!("w{nonce}-dind"),
            (None, WorkerContainerRole::Runner) => format!("{}-runner", self.private_volume),
            (None, WorkerContainerRole::Dind) => format!("{}-dind", self.private_volume),
        }
    }

    pub(crate) fn remote_runner_name(&self) -> Result<String, HostError> {
        self.reservation_nonce
            .as_deref()
            .map(|nonce| format!("v{nonce}"))
            .ok_or(HostError::Identity)
    }
}

/// One of the two fixed bootstrap containers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerContainerRole {
    /// Unprivileged Actions runner.
    Runner,
    /// Private privileged `DinD` service.
    Dind,
}

fn valid_private_volume(private_volume: &str) -> bool {
    !private_volume.is_empty()
        && private_volume
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole};

    const NONCE: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn one_reservation_nonce_formats_remote_runner_containers_and_volumes() {
        let names = WorkerIdentityNames::from_nonce(NONCE).expect("correct reservation nonce");

        assert_eq!(names.private_volume(), format!("w{NONCE}"));
        assert_eq!(
            names.remote_runner_name().expect("remote runner"),
            format!("v{NONCE}")
        );
        assert_eq!(
            names.container_name(WorkerContainerRole::Runner),
            format!("w{NONCE}-runner")
        );
        assert_eq!(
            names.container_name(WorkerContainerRole::Dind),
            format!("w{NONCE}-dind")
        );
        assert_eq!(
            names.volume_name(WorkerVolumeRole::Socket),
            format!("w{NONCE}")
        );
        assert_eq!(
            names.volume_name(WorkerVolumeRole::Work),
            format!("w{NONCE}-work")
        );
        assert_eq!(
            names.volume_name(WorkerVolumeRole::DindData),
            format!("w{NONCE}-docker")
        );
        assert_eq!(
            names.volume_name(WorkerVolumeRole::RunnerMetadata),
            format!("w{NONCE}-metadata")
        );
    }

    #[test]
    fn nonce_and_existing_private_volume_validation_are_exact() {
        assert!(WorkerIdentityNames::from_nonce("A123456789abcdef0123456789abcdef").is_err());
        assert!(WorkerIdentityNames::from_nonce("0123456789abcdef").is_err());
        assert!(WorkerIdentityNames::from_private_volume("w../escape").is_err());
        assert_eq!(
            WorkerIdentityNames::from_private_volume(&format!("w{NONCE}"))
                .expect("recorded volume")
                .remote_runner_name()
                .expect("same nonce"),
            format!("v{NONCE}")
        );
        assert!(
            WorkerIdentityNames::from_private_volume("wtransport")
                .expect("legacy cleanup identity")
                .remote_runner_name()
                .is_err()
        );
    }
}
