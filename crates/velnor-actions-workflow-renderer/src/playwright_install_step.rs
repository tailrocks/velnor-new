//! The Playwright browser install step (hard decision 6): an internal
//! operation of the staged Velnor binary, never a shell and never
//! `playwright install`. It holds no credential; the pins it carries are the
//! literal expectations the operation recomputes from the checked-out locks.
//! A job whose browsers include `WebKit` or Firefox also names them in
//! [`PLAYWRIGHT_SYSTEM_DEPS_ENV`]: the operation then installs their system
//! libraries with Playwright's own `install-deps`, run from the
//! integrity-verified `playwright-core` tarball (tailrocks/velnor-new#352).

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
/// Env key naming the browsers (sorted, comma-joined) whose system libraries
/// the step installs; absent when the hosted image carries them (Chromium).
pub const PLAYWRIGHT_SYSTEM_DEPS_ENV: &str = "VELNOR_PLAYWRIGHT_SYSTEM_DEPS";
/// Browsers the hosted `ubuntu-26.04` image lacks system libraries for.
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

/// Build the install step for one member.
///
/// `pins` is the sorted, comma-joined `directory:sha256` list; `browsers` are
/// the job's Playwright browser names, of which the ones in
/// [`PLAYWRIGHT_SYSTEM_DEPS_BROWSERS`] get their system libraries installed.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for a malformed version, root or pin
/// list, pins that are empty, unsorted or repeated, or browsers that are
/// unknown, unsorted or repeated.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// tailrocks/velnor-new#352: `WebKit` and Firefox get their system
    /// libraries, Chromium (carried by the hosted image) does not.
    #[test]
    fn webkit_and_firefox_jobs_name_their_system_dependencies() -> Result<(), RenderError> {
        let pins = format!("webkit-2370:{}", "c".repeat(64));
        let deps = |browsers: &[&str]| -> Result<Option<String>, RenderError> {
            let step = playwright_install_step("frontend", "1.64.1", &pins, browsers)?;
            let StepKind::Internal { env, .. } = &step.kind else {
                return Err(RenderError::BadCommand("not internal".to_owned()));
            };
            Ok(env.get(PLAYWRIGHT_SYSTEM_DEPS_ENV).cloned())
        };
        assert_eq!(deps(&["chromium"])?, None);
        assert_eq!(deps(&["chromium", "webkit"])?.as_deref(), Some("webkit"));
        assert_eq!(
            deps(&["chromium", "firefox", "webkit"])?.as_deref(),
            Some("firefox,webkit")
        );
        Ok(())
    }
}
