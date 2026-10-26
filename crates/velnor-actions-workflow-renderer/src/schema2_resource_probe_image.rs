// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::ReleaseTarget;

use crate::RenderError;
use crate::yaml::Yaml;

use super::{ProductReleasePins, generator_release};

pub(super) fn build_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    Ok(vec![
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned resource-probe build and qualification tools",
            &pins.install_resource_probe_release_tools_argv,
        )?,
        generator_release::command_step(
            "Install pinned Linux musl target",
            &pins.install_resource_probe_target_argv,
        )?,
        generator_release::verify_rust_step(pins)?,
        generator_release::command_step(
            "Build locked resource probe",
            &pins.resource_probe_build_argv,
        )?,
        generator_release::command_step(
            "Qualify public resource-probe release admission",
            &pins.resource_probe_release_test_argv,
        )?,
        generator_release::command_step(
            "Build locked native resource-probe producer",
            &pins.resource_probe_release_build_argv,
        )?,
        generator_release::bash_step(
            "Build and smoke-test resource-probe image",
            r#"set -euo pipefail
producer_target="$(cd -- "${CARGO_TARGET_DIR:-crates/velnor-runner/target}" && pwd -P)"
export VELNOR_RESOURCE_PROBE_RELEASE_BIN="$producer_target/x86_64-unknown-linux-gnu/debug/velnor-resource-probe-release"
bash images/resource-probe/build-image.sh"#,
        ),
    ])
}
