// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{ProductReleasePins, generator_release};
use crate::{RenderError, commands::join_argv_for_run, yaml::Yaml};
use velnor_actions_contract::ReleaseTarget;

pub(super) const BINARY: &str = "$VELNOR_NATIVE_IMAGE_ACTIONS";
pub(super) const CLOSURE: &str = "job-image-native-source-closure.json";

pub(super) fn source_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let script = source_script(&pins.build_argv)?;
    Ok(vec![
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned Rust and MBX",
            &pins.install_build_tools_argv,
        )?,
        generator_release::verify_rust_step(pins)?,
        generator_release::bash_step("Build exact-source native image maintenance", &script),
    ])
}

pub(crate) fn source_script(build: &[String]) -> Result<String, RenderError> {
    let build = join_argv_for_run(build)?;
    Ok(velnor_actions_contract::job_image_candidate_source_script(
        &build,
        velnor_actions_contract::CandidateSourceTarget::Checkout,
    ))
}
