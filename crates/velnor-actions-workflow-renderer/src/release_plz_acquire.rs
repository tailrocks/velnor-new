// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepRole};

use crate::{RenderError, shell_step};

pub const SEMVER_BINARY_PATH: &str = "$RUNNER_TEMP/velnor-semver/cargo-semver-checks";

pub fn acquire_semver_step(runner: &str) -> Result<Step, RenderError> {
    let qualified = velnor_actions_mise::catalog::qualified_cargo_semver_checks(runner)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unqualified_cargo_semver_checks:{runner}"))
        })?;
    let version = qualified.version;
    let target = qualified.target;
    let archive_sha = qualified.archive_sha256;
    let binary_sha = qualified.executable_sha256;
    let script = format!(
        r#"set -eu; umask 077; test ! -e "$RUNNER_TEMP/velnor-semver"; /bin/mkdir "$RUNNER_TEMP/velnor-semver"; /usr/bin/curl -fsSL --connect-timeout 15 --max-time 120 --max-filesize 104857600 --retry 5 --retry-all-errors --proto '=https' --tlsv1.2 'https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v{version}/cargo-semver-checks-{target}.tar.gz' -o "$RUNNER_TEMP/velnor-semver/archive.tar.gz"; printf '%s  %s\n' '{archive_sha}' "$RUNNER_TEMP/velnor-semver/archive.tar.gz" | /usr/bin/sha256sum -c -; /usr/bin/tar -tzf "$RUNNER_TEMP/velnor-semver/archive.tar.gz" > "$RUNNER_TEMP/velnor-semver/entries"; printf 'cargo-semver-checks\n' | /usr/bin/cmp -s - "$RUNNER_TEMP/velnor-semver/entries"; /usr/bin/tar -xzf "$RUNNER_TEMP/velnor-semver/archive.tar.gz" -C "$RUNNER_TEMP/velnor-semver" cargo-semver-checks; test -f "{SEMVER_BINARY_PATH}"; test ! -L "{SEMVER_BINARY_PATH}"; printf '%s  %s\n' '{binary_sha}' "{SEMVER_BINARY_PATH}" | /usr/bin/sha256sum -c -; /bin/chmod 500 "{SEMVER_BINARY_PATH}"; "{SEMVER_BINARY_PATH}" --version > "$RUNNER_TEMP/velnor-semver/version"; printf 'cargo-semver-checks {version}\n' | /usr/bin/cmp -s - "$RUNNER_TEMP/velnor-semver/version""#
    );
    let mut step = shell_step(
        "Acquire cargo-semver-checks",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )?;
    step.role = Some(StepRole::AcquireCargoSemverChecks);
    Ok(step)
}

pub const BINARY_PATH: &str = "$RUNNER_TEMP/velnor-release-plz/release-plz";

pub fn acquire_step(version: &str, runner: &str) -> Result<Step, RenderError> {
    let qualified = velnor_actions_mise::catalog::qualified_release_plz(version, runner)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unqualified_release_plz:{version}:{runner}"))
        })?;
    let target = qualified.target;
    let archive_sha = qualified.archive_sha256;
    let binary_sha = qualified.executable_sha256;
    let digest = qualified.checksum_command;
    let url = format!(
        "https://github.com/release-plz/release-plz/releases/download/release-plz-v{version}/release-plz-{target}.tar.gz"
    );
    let script = format!(
        "set -eu; umask 077; test ! -e \"$RUNNER_TEMP/velnor-release-plz\"; /bin/mkdir \"$RUNNER_TEMP/velnor-release-plz\"; /usr/bin/curl -fsSL --connect-timeout 15 --max-time 120 --retry 5 --retry-all-errors --proto '=https' --tlsv1.2 '{url}' -o \"$RUNNER_TEMP/velnor-release-plz/archive.tar.gz\"; printf '%s  %s\\n' '{archive_sha}' \"$RUNNER_TEMP/velnor-release-plz/archive.tar.gz\" | {digest} -c -; /usr/bin/tar -tzf \"$RUNNER_TEMP/velnor-release-plz/archive.tar.gz\" > \"$RUNNER_TEMP/velnor-release-plz/entries\"; printf 'release-plz\\n' | /usr/bin/cmp -s - \"$RUNNER_TEMP/velnor-release-plz/entries\"; /usr/bin/tar -xzf \"$RUNNER_TEMP/velnor-release-plz/archive.tar.gz\" -C \"$RUNNER_TEMP/velnor-release-plz\" release-plz; test -f \"{BINARY_PATH}\"; test ! -L \"{BINARY_PATH}\"; printf '%s  %s\\n' '{binary_sha}' \"{BINARY_PATH}\" | {digest} -c -; /bin/chmod 500 \"{BINARY_PATH}\""
    );
    let mut step = shell_step(
        "Acquire release-plz",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )?;
    step.role = Some(StepRole::AcquireReleasePlz);
    Ok(step)
}
