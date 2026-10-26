// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::playwright_lock::{
    PLAYWRIGHT_INSTALL_OPERATION, is_browser_directory,
};
use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::RenderError;

pub const PLAYWRIGHT_INSTALL_STEP_NAME: &str = "Install Playwright browsers";
pub const PLAYWRIGHT_VERSION_ENV: &str = "VELNOR_PLAYWRIGHT_VERSION";
pub const PLAYWRIGHT_PINS_ENV: &str = "VELNOR_PLAYWRIGHT_PINS";
pub const PLAYWRIGHT_ROOT_ENV: &str = "VELNOR_PLAYWRIGHT_ROOT";
pub const PLAYWRIGHT_SYSTEM_DEPS_ENV: &str = "VELNOR_PLAYWRIGHT_SYSTEM_DEPS";
pub const PLAYWRIGHT_SYSTEM_DEPS_BROWSERS: [&str; 2] = ["firefox", "webkit"];

fn bad(detail: &str) -> RenderError {
    RenderError::BadCommand(format!("playwright_install:{detail}"))
}

fn valid_pin(pin: &str) -> bool {
    pin.split_once(':').is_some_and(|(directory, sha256)| {
        is_browser_directory(directory)
            && sha256.len() == 64
            && sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

pub fn playwright_install_step(
    root: &str,
    version: &str,
    pins: &str,
    browsers: &[&str],
) -> Result<Step, RenderError> {
    if !browsers.windows(2).all(|pair| pair[0] < pair[1])
        || !browsers
            .iter()
            .all(|name| matches!(*name, "chromium" | "firefox" | "webkit"))
    {
        return Err(bad("browsers"));
    }
    let system_deps: Vec<&str> = browsers
        .iter()
        .copied()
        .filter(|name| PLAYWRIGHT_SYSTEM_DEPS_BROWSERS.contains(name))
        .collect();
    if !velnor_actions_contract::config::is_exact_semver(version) {
        return Err(bad("version"));
    }
    let root = if root.is_empty() { "." } else { root };
    if root != "." && velnor_actions_contract::validate_fetch_root(root).is_err() {
        return Err(bad("root"));
    }
    let list: Vec<&str> = pins.split(',').collect();
    let directory = |pin: &str| pin.split_once(':').map_or("", |(name, _)| name).to_owned();
    if !list.iter().all(|pin| valid_pin(pin))
        || !list
            .windows(2)
            .all(|pair| directory(pair[0]) < directory(pair[1]))
    {
        return Err(bad("pins"));
    }
    let mut env = BTreeMap::from([
        (PLAYWRIGHT_VERSION_ENV.to_owned(), version.to_owned()),
        (PLAYWRIGHT_ROOT_ENV.to_owned(), root.to_owned()),
        (PLAYWRIGHT_PINS_ENV.to_owned(), pins.to_owned()),
    ]);
    if !system_deps.is_empty() {
        env.insert(PLAYWRIGHT_SYSTEM_DEPS_ENV.to_owned(), system_deps.join(","));
    }
    Ok(Step {
        parallel: false,
        name: PLAYWRIGHT_INSTALL_STEP_NAME.to_owned(),
        id: None,
        role: Some(StepRole::PlaywrightInstall),
        condition: None,
        kind: StepKind::Internal {
            operation: PLAYWRIGHT_INSTALL_OPERATION.to_owned(),
            env,
        },
    })
}
