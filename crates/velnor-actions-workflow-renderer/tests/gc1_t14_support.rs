// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::Value;
use velnor_actions_workflow_renderer::yaml::render_yaml;

#[path = "../src/cache_steps_tools_outputs.rs"]
mod outputs;
#[path = "../src/cache_steps_tools_verify.rs"]
mod verify;

pub(super) fn outputs() -> Result<Value, Box<dyn Error>> {
    Ok(serde_saphyr::from_str(&render_yaml(
        &outputs::action_outputs(),
    ))?)
}

pub(super) fn step(pins: &[String], producer: bool) -> Result<Value, Box<dyn Error>> {
    let step = if producer {
        verify::producer_verify_step(pins)?
    } else {
        verify::installs_verify_step(pins)?
    };
    Ok(serde_saphyr::from_str(&render_yaml(&step))?)
}

pub(super) fn reject_pin(pin: &String) -> Result<(), Box<dyn Error>> {
    for producer in [false, true] {
        match step(std::slice::from_ref(pin), producer) {
            Err(error) if error.to_string().contains("bad_tool_pin") => {}
            result => {
                return Err(super::fail(&format!(
                    "invalid tool pin not rejected: {result:?}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn run(
    step: &Value,
    root: &std::path::Path,
    output: &std::path::Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    let env = |name: &str| {
        step["env"][name]
            .as_str()
            .ok_or_else(|| super::fail(&format!("rendered env missing: {name}")))
    };
    Ok(std::process::Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-c", super::script(step)?])
        .env_clear()
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("ALL_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "")
        .env("HOME", root)
        .env("GITHUB_WORKSPACE", root)
        .env("GITHUB_OUTPUT", output)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("VELNOR_TOOL_PRODUCER", env("VELNOR_TOOL_PRODUCER")?)
        .env("VELNOR_TOOL_PINS", env("VELNOR_TOOL_PINS")?)
        .output()?)
}
