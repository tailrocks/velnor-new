// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_contract::config::playwright_lock::{LockedArchive, PLAYWRIGHT_HOST_PLATFORM};
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::PLAYWRIGHT_SYSTEM_DEPS_BROWSERS;

use super::playwright_install::{Downloader, InstallRequest};
use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{ArchiveBudget, extract_archive};
use crate::internal::internal;

const NPM_REGISTRY: &str = "https://registry.npmjs.org";
const CORE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const CORE_SUBDIR: &str = "velnor/playwright-core";
const EXTRACT_TIMEOUT: Duration = Duration::from_mins(5);
const INSTALL_TIMEOUT: Duration = Duration::from_mins(8);
const INSTALL_ATTEMPTS: u32 = 2;
const CAPTURE_BYTES: usize = 1024 * 1024;
pub(crate) const BUN_VERSION_ENV: &str = "VELNOR_TOOL_BUN";

pub(crate) fn parse_system_deps(value: Option<&str>) -> Result<Vec<String>, OrchestratorError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list: Vec<String> = value.split(',').map(str::to_owned).collect();
    let known = list
        .iter()
        .all(|name| PLAYWRIGHT_SYSTEM_DEPS_BROWSERS.contains(&name.as_str()));
    if !known || !list.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(internal("playwright_system_deps_invalid"));
    }
    Ok(list)
}

pub(crate) trait DepsInstaller {
    fn install_deps(&self, cli: &Path, browsers: &[String]) -> Result<(), OrchestratorError>;
}

pub(crate) struct BunInstallDeps {
    pub(crate) bun_version: String,
}

impl DepsInstaller for BunInstallDeps {
    fn install_deps(&self, cli: &Path, browsers: &[String]) -> Result<(), OrchestratorError> {
        if !velnor_actions_contract::config::is_exact_semver(&self.bun_version) {
            return Err(internal("playwright_system_deps_bun_version"));
        }
        let catalog = ToolCatalog::pinned()
            .with_project_tool(PinnedTool::Bun, &self.bun_version)
            .map_err(|err| internal(&format!("playwright_system_deps_bun:{err}")))?;
        let mut args = vec![cli.as_os_str().to_owned(), OsString::from("install-deps")];
        args.extend(browsers.iter().map(OsString::from));
        let command = PinnedToolExec::new(vec![PinnedTool::Bun], OsStr::new("bun"), args)
            .and_then(|exec| exec.command(&catalog))
            .and_then(|command| {
                command.with_env(&[(
                    OsString::from("PLAYWRIGHT_HOST_PLATFORM_OVERRIDE"),
                    OsString::from(PLAYWRIGHT_HOST_PLATFORM),
                )])
            })
            .map_err(|err| internal(&format!("playwright_system_deps_command:{err}")))?;
        install_with_retry(INSTALL_ATTEMPTS, &mut || {
            let deadline = CheckDeadline::after(INSTALL_TIMEOUT)?;
            command
                .run_until_with_progress(CAPTURE_BYTES, deadline, &mut std::io::stderr().lock())
                .map(|output| output.success)
        })
    }
}

pub(crate) fn install_with_retry(
    attempts: u32,
    run: &mut dyn FnMut() -> Result<bool, velnor_actions_mise::MiseError>,
) -> Result<(), OrchestratorError> {
    let mut failure = internal("playwright_system_deps_failed");
    for attempt in 1..=attempts {
        failure = match run() {
            Ok(true) => return Ok(()),
            Ok(false) => internal("playwright_system_deps_failed"),
            Err(err) if velnor_actions_mise::command::is_cleanup_failure(&err) => {
                return Err(internal(&format!("playwright_system_deps_spawn:{err}")));
            }
            Err(err) => internal(&format!("playwright_system_deps_spawn:{err}")),
        };
        if attempt < attempts {
            eprintln!(
                "velnor: playwright install-deps attempt {attempt} failed ({failure}); retrying"
            );
        }
    }
    Err(failure)
}

fn io_error(path: &Path, err: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), err.to_string())
}

pub(crate) fn install_system_deps(
    request: &InstallRequest,
    archives: &[&LockedArchive],
    downloader: &dyn Downloader,
    installer: &dyn DepsInstaller,
) -> Result<(), OrchestratorError> {
    if request.system_deps.is_empty() {
        return Ok(());
    }
    for browser in &request.system_deps {
        if !archives
            .iter()
            .any(|archive| archive.browsers.contains(browser))
        {
            return Err(internal(&format!(
                "playwright_system_deps_unpinned:{browser}"
            )));
        }
    }
    let integrity = archives
        .first()
        .map(|archive| archive.core_integrity.as_str())
        .ok_or_else(|| internal("playwright_lock_missing"))?;
    let work: PathBuf = request.runner_temp.join(CORE_SUBDIR).join(&request.version);
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|err| io_error(&work, &err))?;
    }
    fs::create_dir_all(&work).map_err(|err| io_error(&work, &err))?;
    let url = format!(
        "{NPM_REGISTRY}/playwright-core/-/playwright-core-{}.tgz",
        request.version
    );
    let tarball = work.join("core.tgz");
    downloader.download(&url, &tarball, CORE_MAX_BYTES)?;
    super::playwright_resolve::verify_integrity(&tarball, integrity)?;
    let extracted = work.join("extracted");
    let deadline =
        CheckDeadline::after(EXTRACT_TIMEOUT).map_err(|err| internal(&err.to_string()))?;
    extract_archive(
        &tarball,
        &extracted,
        &url,
        &mut ArchiveBudget::new(),
        deadline,
    )?;
    installer.install_deps(
        &extracted.join("package").join("cli.js"),
        &request.system_deps,
    )?;
    println!(
        "velnor: playwright system dependencies ready ({})",
        request.system_deps.join(", ")
    );
    Ok(())
}
