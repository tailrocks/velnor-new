// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;

const NAMES: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
];

pub fn refuse_cpu_flags(environment: &[(String, String)]) -> Result<String, CiError> {
    for (name, value) in environment {
        if NAMES.contains(&name.as_str()) {
            let flags = value.replace('_', "-");
            if flags.contains("target-cpu") || flags.contains("target-feature") {
                return Err(CiError::new(format!(
                    "{name} sets target-cpu or target-feature, which would make the published linux/amd64 binary run only on the CPUs of this runner; remove it from the runner environment"
                )));
            }
        }
    }
    Ok("no CPU-specific rustflags in the environment\n".to_owned())
}
