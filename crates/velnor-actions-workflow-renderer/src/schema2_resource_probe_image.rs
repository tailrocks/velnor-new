//! Typed pinned build steps for the source-bound Linux resource-probe image.

use velnor_actions_contract::ReleaseTarget;

use crate::RenderError;
use crate::yaml::Yaml;

use super::{ProductReleasePins, generator_release};

pub(super) fn build_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    Ok(vec![
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned Rust and MBX for the resource probe",
            &pins.install_runner_build_tools_argv,
        )?,
        generator_release::command_step(
            "Install pinned Linux musl target",
            &pins.install_resource_probe_target_argv,
        )?,
        generator_release::command_step(
            "Build locked resource probe through MBX",
            &pins.resource_probe_build_argv,
        )?,
        generator_release::bash_step(
            "Build and smoke-test resource-probe image",
            "bash images/resource-probe/build-image.sh",
        ),
    ])
}
