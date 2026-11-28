// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectError {
    Config,
    Lock,
    Rejected,
    Secret(HostError),
    Lifecycle(HostError),
    Policy(HostError),
    Write,
    WriteUncertain,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config => formatter.write_str("invalid config"),
            Self::Lock => formatter.write_str("config is locked"),
            Self::Rejected => formatter.write_str("rejected connection"),
            Self::Secret(error) | Self::Lifecycle(error) | Self::Policy(error) => {
                std::fmt::Display::fmt(error, formatter)
            }
            Self::Write => formatter.write_str("write failed"),
            Self::WriteUncertain => formatter.write_str("write state uncertain"),
        }
    }
}

impl ConnectError {
    pub(super) const fn from_config(error: HostError) -> Self {
        match error {
            HostError::JobAuthorityPolicySchema { .. } => Self::Policy(error),
            _ => Self::Config,
        }
    }
}
