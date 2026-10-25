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
    PLAYWRIGHT_PINS_ENV, PLAYWRIGHT_ROOT_ENV, PLAYWRIGHT_SYSTEM_DEPS_ENV, PLAYWRIGHT_VERSION_ENV,
};

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{ArchiveBudget, extract_archive};
use crate::config::playwright_lock::{hash_file, read_playwright_lock};
use crate::internal::internal;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

pub const PLAYWRIGHT_INSTALL_OP: &str =
    velnor_actions_contract::config::playwright_lock::PLAYWRIGHT_INSTALL_OPERATION;
pub(crate) const BROWSERS_SUBDIR: &str = "velnor/ms-playwright";
const DOWNLOAD_SUBDIR: &str = "velnor/playwright-download";
const COMPLETE_MARKER: &str = "INSTALLATION_COMPLETE";
const PIN_MARKER: &str = "velnor-sha256";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(30);
const EXTRACT_TIMEOUT: Duration = Duration::from_mins(30);
const CAPTURE_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InstallRequest {
    pub(crate) version: String,
    pub(crate) pins: Vec<(String, String)>,
    pub(crate) root: String,
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
    pub(crate) system_deps: Vec<String>,
}

impl InstallRequest {
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
            system_deps: super::playwright_system_deps::parse_system_deps(
                get(PLAYWRIGHT_SYSTEM_DEPS_ENV).as_deref(),
            )?,
        })
    }

    fn browsers_dir(&self) -> PathBuf {
        self.runner_temp.join(BROWSERS_SUBDIR)
    }

    fn download_dir(&self) -> PathBuf {
        self.runner_temp.join(DOWNLOAD_SUBDIR)
    }
}

pub(crate) trait Downloader {
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

fn is_installed(directory: &Path, sha256: &str) -> bool {
    directory.join(COMPLETE_MARKER).is_file()
        && fs::read_to_string(directory.join(PIN_MARKER)).is_ok_and(|text| text.trim() == sha256)
}

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
    if !request.system_deps.is_empty() {
        let bun_version = get(super::playwright_system_deps::BUN_VERSION_ENV)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("playwright_missing_env:VELNOR_TOOL_BUN"))?;
        super::playwright_system_deps::install_system_deps(
            &request,
            &archives,
            &Curl,
            &super::playwright_system_deps::BunInstallDeps { bun_version },
        )?;
    }
    let fetched = install(&request, &archives, &Curl)?;
    println!(
        "velnor: playwright browsers ready ({} archives, {fetched} downloaded)",
        archives.len()
    );
    Ok(())
}
