// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha512};
use velnor_actions_contract::config::playwright_lock::{
    LockedArchive, PLAYWRIGHT_HOST_PLATFORM, PLAYWRIGHT_LOCK_PATH, PLAYWRIGHT_LOCK_SCHEMA,
    PLAYWRIGHT_MAX_ARCHIVE_BYTES, PlaywrightLock, is_browser_directory, is_playwright_download_url,
};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand, PinnedTool, PinnedToolExec};

use super::playwright_lock::{CoreRequest, core_requests, hash_file, render_playwright_lock};
use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{ArchiveBudget, extract_archive};
use crate::config::{config_error, load_config};
use crate::internal::internal;

const NPM_REGISTRY: &str = "https://registry.npmjs.org";
const CORE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(30);
const DRY_RUN_TIMEOUT: Duration = Duration::from_mins(2);
const CAPTURE_BYTES: usize = 1024 * 1024;

pub(crate) trait BrowserSource {
    fn download_core(&self, version: &str, dest: &Path) -> Result<(), OrchestratorError>;

    fn dry_run(&self, core_dir: &Path, browser: &str) -> Result<String, OrchestratorError>;

    fn download_archive(&self, url: &str, dest: &Path) -> Result<(), OrchestratorError>;
}

pub(crate) fn parse_dry_run(output: &str) -> Result<Vec<(String, String)>, OrchestratorError> {
    let mut location: Option<String> = None;
    let mut found = Vec::new();
    for line in output.lines().map(str::trim) {
        if let Some(path) = line.strip_prefix("Install location:") {
            location = Path::new(path.trim())
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
        } else if let Some(url) = line.strip_prefix("Download url:") {
            let directory = location
                .take()
                .ok_or_else(|| internal("playwright_dry_run_no_location"))?;
            let url = url.trim().to_owned();
            if !is_browser_directory(&directory) || !is_playwright_download_url(&url) {
                return Err(internal(&format!(
                    "playwright_dry_run_rejected:{directory}"
                )));
            }
            found.push((directory, url));
        }
    }
    if found.is_empty() {
        return Err(internal("playwright_dry_run_empty"));
    }
    Ok(found)
}

pub(crate) fn verify_integrity(path: &Path, expected: &str) -> Result<(), OrchestratorError> {
    let bytes = fs::read(path)
        .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
    let actual = format!("sha512-{}", STANDARD.encode(Sha512::digest(&bytes)));
    if actual == expected {
        Ok(())
    } else {
        Err(internal("playwright_core_integrity_mismatch"))
    }
}

pub(crate) fn build_playwright_lock(
    requests: &[CoreRequest],
    source: &dyn BrowserSource,
    scratch: &Path,
) -> Result<PlaywrightLock, OrchestratorError> {
    let mut archives: BTreeMap<(String, String), LockedArchive> = BTreeMap::new();
    for request in requests {
        let work = scratch.join(format!("core-{}", request.version));
        let tarball = work.join("core.tgz");
        let extracted = work.join("extracted");
        let package = extracted.join("package");
        fs::create_dir_all(&work)
            .map_err(|err| OrchestratorError::io(work.display().to_string(), err.to_string()))?;
        source.download_core(&request.version, &tarball)?;
        verify_integrity(&tarball, &request.integrity)?;
        let url = format!(
            "{NPM_REGISTRY}/playwright-core/-/playwright-core-{}.tgz",
            request.version
        );
        let deadline =
            CheckDeadline::after(DOWNLOAD_TIMEOUT).map_err(|err| internal(&err.to_string()))?;
        extract_archive(
            &tarball,
            &extracted,
            &url,
            &mut ArchiveBudget::new(),
            deadline,
        )?;
        let mut found: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
        for browser in &request.browsers {
            let listing = source.dry_run(&package, browser.as_str())?;
            for (directory, url) in parse_dry_run(&listing)? {
                let entry = found
                    .entry(directory.clone())
                    .or_insert_with(|| (url.clone(), BTreeSet::new()));
                if entry.0 != url {
                    return Err(internal(&format!("playwright_url_conflict:{directory}")));
                }
                entry.1.insert(browser.as_str().to_owned());
            }
        }
        for (directory, (url, browsers)) in found {
            let file = work.join(format!("{directory}.zip"));
            source.download_archive(&url, &file)?;
            let (sha256, bytes) = hash_file(&file, PLAYWRIGHT_MAX_ARCHIVE_BYTES)?;
            archives.insert(
                (request.version.clone(), directory.clone()),
                LockedArchive {
                    version: request.version.clone(),
                    core_integrity: request.integrity.clone(),
                    browsers: browsers.into_iter().collect(),
                    directory,
                    url,
                    sha256,
                    bytes,
                },
            );
        }
    }
    let lock = PlaywrightLock {
        schema: PLAYWRIGHT_LOCK_SCHEMA,
        platform: PLAYWRIGHT_HOST_PLATFORM.to_owned(),
        archives: archives.into_values().collect(),
    };
    lock.validate(PLAYWRIGHT_LOCK_PATH).map_err(config_error)?;
    Ok(lock)
}

pub fn lock_playwright_browsers(root: &Path) -> Result<String, OrchestratorError> {
    let config = load_config(root)?;
    let Some(bun) = &config.stacks.bun else {
        return Err(OrchestratorError::config(
            crate::config::CONFIG_REL,
            "stacks.bun",
            "missing_bun_stack",
        ));
    };
    let inventory =
        crate::workflow_task_jobs::bun_inventory::BunInventory::load_unindexed(root, bun)?;
    let requests = core_requests(bun, &inventory)?;
    if requests.is_empty() {
        return Err(OrchestratorError::config(
            crate::config::CONFIG_REL,
            "stacks.bun.members.browsers",
            "no_browsers_declared",
        ));
    }
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(root, &config)?;
    let scratch = tempfile::tempdir()
        .map_err(|err| OrchestratorError::io("playwright lock scratch", err.to_string()))?;
    let scratch_path = scratch
        .path()
        .canonicalize()
        .map_err(|err| OrchestratorError::io("playwright lock scratch", err.to_string()))?;
    let lock = build_playwright_lock(&requests, &NetworkSource { catalog }, &scratch_path)?;
    let text = render_playwright_lock(&lock)?;
    crate::config::deps_lock::write_repo_file(root, PLAYWRIGHT_LOCK_PATH, &text)?;
    Ok(text)
}

struct NetworkSource {
    catalog: velnor_actions_mise::ToolCatalog,
}

fn curl(url: &str, dest: &Path, limit: u64) -> Result<(), OrchestratorError> {
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
        Err(internal(&format!("playwright_download_failed:{url}")))
    }
}

impl BrowserSource for NetworkSource {
    fn download_core(&self, version: &str, dest: &Path) -> Result<(), OrchestratorError> {
        curl(
            &format!("{NPM_REGISTRY}/playwright-core/-/playwright-core-{version}.tgz"),
            dest,
            CORE_MAX_BYTES,
        )
    }

    fn dry_run(&self, core_dir: &Path, browser: &str) -> Result<String, OrchestratorError> {
        let cli: PathBuf = core_dir.join("cli.js");
        let args = vec![
            OsString::from("--bun"),
            cli.into_os_string(),
            OsString::from("install"),
            OsString::from("--dry-run"),
            OsString::from(browser),
        ];
        let exec = PinnedToolExec::new(vec![PinnedTool::Bun], OsStr::new("bun"), args)
            .map_err(|err| internal(&format!("playwright_bun_unavailable:{err}")))?;
        let output = exec
            .command(&self.catalog)
            .and_then(|command| {
                command.with_env(&[(
                    OsString::from("PLAYWRIGHT_HOST_PLATFORM_OVERRIDE"),
                    OsString::from(PLAYWRIGHT_HOST_PLATFORM),
                )])
            })
            .and_then(|command| command.run_bounded(CAPTURE_BYTES, DRY_RUN_TIMEOUT))
            .map_err(|err| internal(&format!("playwright_dry_run_spawn:{err}")))?;
        if !output.success {
            return Err(internal("playwright_dry_run_failed"));
        }
        output
            .stdout_text("bun")
            .map_err(|_| internal("playwright_dry_run_output"))
    }

    fn download_archive(&self, url: &str, dest: &Path) -> Result<(), OrchestratorError> {
        curl(url, dest, PLAYWRIGHT_MAX_ARCHIVE_BYTES)
    }
}
