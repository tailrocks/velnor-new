// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobAuthorityLimit {
    Workflows,
    Jobs,
    Calls,
    Contexts,
    Refs,
    Secrets,
    Environments,
    Needs,
    TextBytes,
    JobKeyBytes,
    EnvironmentBytes,
    PolicyBytes,
}

const WORKFLOWS: usize = 32;
const JOBS: usize = 512;
const CALLS: usize = 64;
const CONTEXTS: usize = 6;
const REFS: usize = 128;
const SECRETS: usize = 128;
const ENVIRONMENTS: usize = 32;
const NEEDS: usize = 256;
const TEXT_BYTES: usize = 512;
const JOB_KEY_BYTES: usize = 100;
const ENVIRONMENT_BYTES: usize = 255;
const POLICY_BYTES: usize = 256 * 1024;

impl JobAuthorityLimit {
    pub const ALL: [Self; 12] = [
        Self::Workflows,
        Self::Jobs,
        Self::Calls,
        Self::Contexts,
        Self::Refs,
        Self::Secrets,
        Self::Environments,
        Self::Needs,
        Self::TextBytes,
        Self::JobKeyBytes,
        Self::EnvironmentBytes,
        Self::PolicyBytes,
    ];

    #[must_use]
    pub const fn max(self) -> usize {
        match self {
            Self::Workflows => WORKFLOWS,
            Self::Jobs => JOBS,
            Self::Calls => CALLS,
            Self::Contexts => CONTEXTS,
            Self::Refs => REFS,
            Self::Secrets => SECRETS,
            Self::Environments => ENVIRONMENTS,
            Self::Needs => NEEDS,
            Self::TextBytes => TEXT_BYTES,
            Self::JobKeyBytes => JOB_KEY_BYTES,
            Self::EnvironmentBytes => ENVIRONMENT_BYTES,
            Self::PolicyBytes => POLICY_BYTES,
        }
    }

    #[must_use]
    pub const fn counts(self) -> &'static str {
        match self {
            Self::Workflows => "workflows",
            Self::Jobs => "jobs",
            Self::Calls => "reusable workflow calls",
            Self::Contexts => "event contexts",
            Self::Refs => "refs",
            Self::Secrets => "secrets",
            Self::Environments => "environments",
            Self::Needs => "needs",
            Self::TextBytes | Self::JobKeyBytes | Self::EnvironmentBytes | Self::PolicyBytes => {
                "bytes"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JobAuthorityLimit as Limit;

    #[test]
    fn the_limits_are_the_documented_promise_of_the_host() {
        let promised = [
            (Limit::Workflows, 32),
            (Limit::Jobs, 512),
            (Limit::Calls, 64),
            (Limit::Contexts, 6),
            (Limit::Refs, 128),
            (Limit::Secrets, 128),
            (Limit::Environments, 32),
            (Limit::Needs, 256),
            (Limit::TextBytes, 512),
            (Limit::JobKeyBytes, 100),
            (Limit::EnvironmentBytes, 255),
            (Limit::PolicyBytes, 262_144),
        ];
        assert_eq!(promised.map(|(limit, _)| limit), Limit::ALL);
        for (limit, max) in promised {
            assert_eq!(limit.max(), max, "{limit:?}");
        }
    }
}
