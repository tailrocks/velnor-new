//! System libraries for Playwright browsers the hosted image does not carry
//! (`playwright-browsers-install-v1`, `VELNOR_PLAYWRIGHT_SYSTEM_DEPS`).
//!
//! The `ubuntu-26.04` hosted image runs Playwright Chromium as is but lacks
//! the `WebKit` and Firefox runtime libraries (tailrocks/velnor-new#352). For
//! those browsers the step runs Playwright's own `install-deps` from the
//! `playwright-core` tarball whose integrity `bun.lock` records: the tarball
//! is downloaded from the npm registry, verified against that integrity (the
//! same anchor as every locked archive) and only then executed under the
//! Velnor-pinned Bun, with the lock's host platform. Repository-resolved code
//! (`node_modules`) never runs here, and the package list is Playwright's
//! `nativeDeps` table for that platform inside the verified tarball.

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
const INSTALL_TIMEOUT: Duration = Duration::from_mins(20);
const CAPTURE_BYTES: usize = 1024 * 1024;
/// Exact Bun version, exported by the project-tool resolver step.
pub(crate) const BUN_VERSION_ENV: &str = "VELNOR_TOOL_BUN";

/// The step's sorted, duplicate-free browser list; absent is none.
///
/// # Errors
/// `playwright_system_deps_invalid` for an empty, unsorted, repeated or
/// unknown entry.
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

/// Runs Playwright's `install-deps`; the real one uses the pinned Bun.
pub(crate) trait DepsInstaller {
    /// Run `<cli> install-deps <browsers>`.
    ///
    /// # Errors
    /// Fails when the command cannot run or exits nonzero.
    fn install_deps(&self, cli: &Path, browsers: &[String]) -> Result<(), OrchestratorError>;
}

/// `mise exec bun@<pin> -- bun <cli> install-deps <browsers>`.
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
        let output = PinnedToolExec::new(vec![PinnedTool::Bun], OsStr::new("bun"), args)
            .and_then(|exec| exec.command(&catalog))
            .and_then(|command| {
                command.with_env(&[(
                    OsString::from("PLAYWRIGHT_HOST_PLATFORM_OVERRIDE"),
                    OsString::from(PLAYWRIGHT_HOST_PLATFORM),
                )])
            })
            .and_then(|command| command.run_bounded(CAPTURE_BYTES, INSTALL_TIMEOUT))
            .map_err(|err| internal(&format!("playwright_system_deps_spawn:{err}")))?;
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        if output.success {
            Ok(())
        } else {
            Err(internal("playwright_system_deps_failed"))
        }
    }
}

fn io_error(path: &Path, err: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), err.to_string())
}

/// Install the system libraries of `request.system_deps`.
///
/// Each browser must be one the verified archives pin, and the tarball must
/// match their `core_integrity` (already checked equal to `bun.lock`'s)
/// before anything in it runs.
///
/// # Errors
/// `playwright_system_deps_unpinned:<browser>`, a download failure,
/// `playwright_core_integrity_mismatch`, an unsafe tarball or a failed install.
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

#[cfg(test)]
#[path = "playwright_system_deps_tests.rs"]
mod tests;
