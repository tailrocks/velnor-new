//! The Playwright browser install step (hard decision 6): an internal
//! operation of the staged Velnor binary, never a shell and never
//! `playwright install`. It holds no credential; the pins it carries are the
//! literal expectations the operation recomputes from the checked-out locks.

use std::collections::BTreeMap;

use velnor_actions_contract::config::playwright_lock::{
    PLAYWRIGHT_INSTALL_OPERATION, is_browser_directory,
};
use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::RenderError;

/// Display name of the install step.
pub const PLAYWRIGHT_INSTALL_STEP_NAME: &str = "Install Playwright browsers";
/// Env key holding the Playwright version the member resolved.
pub const PLAYWRIGHT_VERSION_ENV: &str = "VELNOR_PLAYWRIGHT_VERSION";
/// Env key holding the literal `directory:sha256` pin list.
pub const PLAYWRIGHT_PINS_ENV: &str = "VELNOR_PLAYWRIGHT_PINS";
/// Env key naming the workspace root whose `bun.lock` anchors the browser lock.
pub const PLAYWRIGHT_ROOT_ENV: &str = "VELNOR_PLAYWRIGHT_ROOT";

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

/// Build the install step for one member.
///
/// `pins` is the sorted, comma-joined `directory:sha256` list.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for a malformed version, root or pin
/// list, or pins that are empty, unsorted or repeated.
pub fn playwright_install_step(root: &str, version: &str, pins: &str) -> Result<Step, RenderError> {
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
    Ok(Step {
        parallel: false,
        name: PLAYWRIGHT_INSTALL_STEP_NAME.to_owned(),
        id: None,
        role: Some(StepRole::PlaywrightInstall),
        condition: None,
        kind: StepKind::Internal {
            operation: PLAYWRIGHT_INSTALL_OPERATION.to_owned(),
            env: BTreeMap::from([
                (PLAYWRIGHT_VERSION_ENV.to_owned(), version.to_owned()),
                (PLAYWRIGHT_ROOT_ENV.to_owned(), root.to_owned()),
                (PLAYWRIGHT_PINS_ENV.to_owned(), pins.to_owned()),
            ]),
        },
    })
}
