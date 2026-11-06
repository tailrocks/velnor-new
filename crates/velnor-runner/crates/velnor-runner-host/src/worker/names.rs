// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeRole {
    Socket,

    Home,

    Work,

    DindData,

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkerIdentityNames {
    private_volume: String,
    reservation_nonce: Option<String>,
}

impl WorkerIdentityNames {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerContainerRole {
    Runner,

    Dind,
}

fn valid_private_volume(private_volume: &str) -> bool {
    !private_volume.is_empty()
        && private_volume
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}
