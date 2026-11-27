// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const RUNNER_USER_AGENT: &str = concat!(
    "actions-runner/",
    velnor_model::runner_protocol_version!(),
    " (velnor)"
);
