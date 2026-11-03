// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::ledger_cli_verify_script::RenderedVerify;
use super::tools_cache_verify_hook::{Sandbox, expand_shell, pair};

pub(super) struct Install<'a> {
    pub(super) dir: &'a str,
    pub(super) version: &'a str,
    pub(super) pins: &'a str,
    pub(super) lock: Option<&'a str>,
    pub(super) archive_sha256: &'a str,
    pub(super) archive: &'a [u8],
    pub(super) extra_archives: &'a [(&'a str, &'a [u8])],
    pub(super) file: &'a [u8],
}

pub(super) struct Outcome {
    pub(super) exit: Option<i32>,
    pub(super) kept: bool,
    pub(super) stdout: String,
}

pub(super) fn run_install(
    step: &RenderedVerify,
    install: &Install<'_>,
    extra: &[(String, String)],
) -> Result<Outcome, Box<dyn Error>> {
    let runner = Sandbox::new()?;
    let installs = runner.mise_root().join("installs");
    let archives = installs.join(".velnor-verified-archives");
    let version = installs.join(install.dir).join(install.version);
    fs::create_dir_all(&archives)?;
    fs::create_dir_all(&version)?;
    fs::write(archives.join(install.archive_sha256), install.archive)?;
    for (sha256, bytes) in install.extra_archives {
        fs::write(archives.join(sha256), bytes)?;
    }
    if let Some(lock) = install.lock {
        fs::write(runner.workspace.path().join("mise.lock"), lock)?;
    }
    fs::write(version.join("file"), install.file)?;
    let mut env = vec![
        pair("VELNOR_TOOL_PINS", install.pins),
        pair("RUNNER_OS", "Linux"),
        pair("RUNNER_ARCH", "X64"),
    ];
    env.extend_from_slice(extra);
    let captured = runner.spawn(&expand_shell(&step.shell), &step.run, &env)?;
    Ok(Outcome {
        exit: captured.code,
        kept: version.exists(),
        stdout: captured.stdout,
    })
}
