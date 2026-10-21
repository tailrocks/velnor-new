//! `playwright-browsers-install-v1`: install the hash-pinned Playwright
//! browsers a Bun member needs (hard decision 6).
//!
//! The step carries the expected `directory:sha256` pins literally. The
//! operation recomputes them from the checked-out `.velnor/playwright.lock`
//! and `bun.lock` and refuses to run on any difference, downloads each archive
//! from the pinned URL, verifies its SHA-256 before extracting, and extracts
//! through the hardened archive reader. No credential is involved and no
//! repository code runs; `playwright install --with-deps` never does.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_bun::parse_lock;
use velnor_actions_contract::config::playwright_lock::{
    PLAYWRIGHT_MAX_ARCHIVE_BYTES, PlaywrightLock, is_browser_directory, is_playwright_download_url,
    playwright_download_candidates,
};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};
use velnor_actions_workflow_renderer::verification_jobs::{
    PLAYWRIGHT_PINS_ENV, PLAYWRIGHT_ROOT_ENV, PLAYWRIGHT_VERSION_ENV,
};

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{ArchiveBudget, extract_archive};
use crate::config::playwright_lock::{hash_file, read_playwright_lock};
use crate::internal::internal;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Operation name (shared with the contract and the renderer).
pub const PLAYWRIGHT_INSTALL_OP: &str =
    velnor_actions_contract::config::playwright_lock::PLAYWRIGHT_INSTALL_OPERATION;
/// Browsers directory (`PLAYWRIGHT_BROWSERS_PATH`) under runner temp.
pub(crate) const BROWSERS_SUBDIR: &str = "velnor/ms-playwright";
const DOWNLOAD_SUBDIR: &str = "velnor/playwright-download";
const COMPLETE_MARKER: &str = "INSTALLATION_COMPLETE";
const PIN_MARKER: &str = "velnor-sha256";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(30);
const EXTRACT_TIMEOUT: Duration = Duration::from_mins(30);
const CAPTURE_BYTES: usize = 64 * 1024;

/// Parsed and validated step input.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InstallRequest {
    pub(crate) version: String,
    /// `(directory, sha256)` sorted by directory.
    pub(crate) pins: Vec<(String, String)>,
    pub(crate) root: String,
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
}

impl InstallRequest {
    /// Parse the step environment.
    ///
    /// # Errors
    /// Rejects missing, relative or malformed values.
    pub(crate) fn parse(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, OrchestratorError> {
        let need = |key: &str| {
            get(key)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| internal(&format!("playwright_missing_env:{key}")))
        };
        let absolute = |key: &str| -> Result<PathBuf, OrchestratorError> {
            let path = PathBuf::from(need(key)?);
            path.is_absolute()
                .then_some(path)
                .ok_or_else(|| internal(&format!("playwright_path_relative:{key}")))
        };
        let version = need(PLAYWRIGHT_VERSION_ENV)?;
        if !velnor_actions_contract::config::is_exact_semver(&version) {
            return Err(internal("playwright_version_invalid"));
        }
        let root = need(PLAYWRIGHT_ROOT_ENV)?;
        let root = if root == "." { String::new() } else { root };
        velnor_actions_contract::validate_fetch_root(&root)
            .map_err(|_| internal("playwright_root_invalid"))?;
        let mut pins = Vec::new();
        for pin in need(PLAYWRIGHT_PINS_ENV)?.split(',') {
            let (directory, sha256) = pin
                .split_once(':')
                .ok_or_else(|| internal("playwright_pin_invalid"))?;
            let hex = sha256.len() == 64
                && sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            if !is_browser_directory(directory) || !hex {
                return Err(internal("playwright_pin_invalid"));
            }
            pins.push((directory.to_owned(), sha256.to_owned()));
        }
        if !pins.windows(2).all(|pair| pair[0].0 < pair[1].0) {
            return Err(internal("playwright_pins_unsorted"));
        }
        Ok(Self {
            version,
            pins,
            root,
            workspace: absolute("GITHUB_WORKSPACE")?,
            runner_temp: absolute("RUNNER_TEMP")?,
        })
    }

    fn browsers_dir(&self) -> PathBuf {
        self.runner_temp.join(BROWSERS_SUBDIR)
    }

    fn download_dir(&self) -> PathBuf {
        self.runner_temp.join(DOWNLOAD_SUBDIR)
    }
}

/// Download transport; the real one is `curl`.
pub(crate) trait Downloader {
    /// Fetch `url` to `dest`, at most `limit` bytes.
    ///
    /// # Errors
    /// Fails when the download does.
    fn download(&self, url: &str, dest: &Path, limit: u64) -> Result<(), OrchestratorError>;
}

struct Curl;

impl Downloader for Curl {
    fn download(&self, url: &str, dest: &Path, limit: u64) -> Result<(), OrchestratorError> {
        let mut args: Vec<OsString> = [
            "--disable",
            "--globoff",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--retry",
            "3",
            "--max-filesize",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        args.push(limit.to_string().into());
        args.extend([OsString::from("--output"), dest.as_os_str().to_owned()]);
        args.push(url.into());
        let output = IsolatedCommand::repo_task("/usr/bin/curl", args, &[])
            .and_then(|command| command.run_bounded(CAPTURE_BYTES, DOWNLOAD_TIMEOUT))
            .map_err(|err| internal(&format!("playwright_download_spawn:{err}")))?;
        if output.success {
            Ok(())
        } else {
            Err(internal("playwright_download_failed"))
        }
    }
}

/// The lock entries the pins name, after the cross-checks against the repository.
///
/// # Errors
/// `playwright_lock_missing`, `playwright_pin_drift:<directory>` and
/// `playwright_core_integrity_drift` fail closed.
pub(crate) fn verified_archives<'a>(
    request: &InstallRequest,
    lock: Option<&'a PlaywrightLock>,
    bun_lock: &str,
) -> Result<
    Vec<&'a velnor_actions_contract::config::playwright_lock::LockedArchive>,
    OrchestratorError,
> {
    let lock = lock.ok_or_else(|| internal("playwright_lock_missing"))?;
    let parsed = parse_lock(bun_lock).map_err(|err| internal(err.code()))?;
    let integrity = parsed.integrity.get("playwright-core");
    let version = parsed.versions.get("playwright-core");
    if version.map(String::as_str) != Some(request.version.as_str()) || integrity.is_none() {
        return Err(internal("playwright_core_integrity_drift"));
    }
    let mut archives = Vec::new();
    for (directory, sha256) in &request.pins {
        let archive = lock
            .archives
            .iter()
            .find(|archive| archive.version == request.version && &archive.directory == directory)
            .filter(|archive| &archive.sha256 == sha256)
            .ok_or_else(|| internal(&format!("playwright_pin_drift:{directory}")))?;
        if Some(&archive.core_integrity) != integrity {
            return Err(internal("playwright_core_integrity_drift"));
        }
        archives.push(archive);
    }
    Ok(archives)
}

fn io_error(path: &Path, err: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), err.to_string())
}

/// Whether `directory` already holds the pinned install (a restored cache).
fn is_installed(directory: &Path, sha256: &str) -> bool {
    directory.join(COMPLETE_MARKER).is_file()
        && fs::read_to_string(directory.join(PIN_MARKER)).is_ok_and(|text| text.trim() == sha256)
}

/// Fetch `archive` into `file` from its locked URL, then try Playwright mirrors for the same artifact.
/// The transport retries each host.
/// The downloaded bytes must match the lock's SHA-256 and size, whichever host supplies them.
fn download_verified(
    downloader: &dyn Downloader,
    archive: &velnor_actions_contract::config::playwright_lock::LockedArchive,
    file: &Path,
) -> Result<(), OrchestratorError> {
    let mut failure = internal("playwright_download_failed");
    for url in playwright_download_candidates(&archive.url) {
        let host = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("?");
        if let Err(err) = downloader.download(&url, file, PLAYWRIGHT_MAX_ARCHIVE_BYTES) {
            eprintln!(
                "velnor: playwright {} download from {host} failed",
                archive.directory
            );
            failure = err;
            continue;
        }
        let (actual, bytes) = hash_file(file, PLAYWRIGHT_MAX_ARCHIVE_BYTES)?;
        if actual == archive.sha256 && bytes == archive.bytes {
            println!(
                "velnor: playwright {} downloaded from {host}",
                archive.directory
            );
            return Ok(());
        }
        fs::remove_file(file).map_err(|err| io_error(file, &err))?;
        eprintln!(
            "velnor: playwright {} from {host} rejected: digest mismatch",
            archive.directory
        );
        failure = internal(&format!("playwright_sha256_mismatch:{}", archive.directory));
    }
    Err(failure)
}

/// Install every pinned archive; returns how many were downloaded.
///
/// # Errors
/// Fails closed on drift, a download failure, a digest mismatch or an unsafe archive.
pub(crate) fn install(
    request: &InstallRequest,
    archives: &[&velnor_actions_contract::config::playwright_lock::LockedArchive],
    downloader: &dyn Downloader,
) -> Result<usize, OrchestratorError> {
    let browsers = request.browsers_dir();
    let downloads = request.download_dir();
    for dir in [&browsers, &downloads] {
        fs::create_dir_all(dir).map_err(|err| io_error(dir, &err))?;
    }
    let mut budget = ArchiveBudget::browser();
    let mut fetched = 0;
    for archive in archives {
        let destination = browsers.join(&archive.directory);
        if is_installed(&destination, &archive.sha256) {
            continue;
        }
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|err| io_error(&destination, &err))?;
        }
        if !is_playwright_download_url(&archive.url) {
            return Err(internal("playwright_url_host"));
        }
        let name = archive.url.clone();
        let file = downloads.join(format!("{}.zip", archive.directory));
        download_verified(downloader, archive, &file)?;
        let partial = browsers.join(format!("{}.partial", archive.directory));
        if partial.exists() {
            fs::remove_dir_all(&partial).map_err(|err| io_error(&partial, &err))?;
        }
        let deadline =
            CheckDeadline::after(EXTRACT_TIMEOUT).map_err(|err| internal(&err.to_string()))?;
        extract_archive(&file, &partial, &name, &mut budget, deadline)?;
        fs::write(partial.join(PIN_MARKER), &archive.sha256)
            .map_err(|err| io_error(&partial, &err))?;
        fs::write(partial.join(COMPLETE_MARKER), b"").map_err(|err| io_error(&partial, &err))?;
        fs::rename(&partial, &destination).map_err(|err| io_error(&destination, &err))?;
        fs::remove_file(&file).map_err(|err| io_error(&file, &err))?;
        fetched += 1;
    }
    Ok(fetched)
}

/// Run `playwright-browsers-install-v1` from the step environment.
///
/// # Errors
/// Any configuration, drift, download or digest failure.
pub fn run_playwright_install() -> Result<(), OrchestratorError> {
    let get = |key: &str| std::env::var(key).ok();
    let request = InstallRequest::parse(&get)?;
    let rel = if request.root.is_empty() {
        "bun.lock".to_owned()
    } else {
        format!("{}/bun.lock", request.root)
    };
    let RepoRead::Text(bun_lock) = read_repo_file(&request.workspace, &rel, MAX_REPO_FILE_BYTES)?
    else {
        return Err(internal("playwright_bun_lock_missing"));
    };
    let lock = read_playwright_lock(&request.workspace)?;
    let archives = verified_archives(&request, lock.as_ref(), &bun_lock)?;
    let fetched = install(&request, &archives, &Curl)?;
    println!(
        "velnor: playwright browsers ready ({} archives, {fetched} downloaded)",
        archives.len()
    );
    Ok(())
}

#[cfg(test)]
#[path = "playwright_install_tests.rs"]
mod tests;
