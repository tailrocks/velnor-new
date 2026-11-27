// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[macro_export]
macro_rules! runner_protocol_version {
    () => {
        "2.338.2"
    };
}

pub const RUNNER_PROTOCOL_VERSION: &str = runner_protocol_version!();
