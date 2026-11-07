// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{base_job, map, step};
use crate::yaml::Yaml;
use velnor_actions_contract::config::ReleaseRehearsalConfig;

pub(super) fn policy_env(config: &ReleaseRehearsalConfig, approved: bool) -> Yaml {
    let mut env = vec![
        ("GH_TOKEN", Yaml::str("${{ github.token }}")),
        (
            "VELNOR_DEFAULT_BRANCH",
            Yaml::str("${{ github.event.repository.default_branch }}"),
        ),
        (
            "VELNOR_REHEARSAL_ENVIRONMENT",
            Yaml::str(&config.environment),
        ),
        (
            "VELNOR_REVIEWER_ID",
            Yaml::str(config.reviewer_id.to_string()),
        ),
    ];
    if approved {
        env.push(("VELNOR_REHEARSAL_STAGE", Yaml::str("approved")));
    }
    map(env)
}
pub(super) fn read_permissions() -> Yaml {
    map(vec![
        ("actions", Yaml::str("read")),
        ("contents", Yaml::str("read")),
    ])
}
pub(super) fn preflight_job(config: &ReleaseRehearsalConfig) -> Yaml {
    let policy = include_str!("release_rehearsal_policy.sh");
    map(base_job(
        vec![step(
            "Verify environment protection",
            policy,
            policy_env(config, false),
        )],
        read_permissions(),
    ))
}
