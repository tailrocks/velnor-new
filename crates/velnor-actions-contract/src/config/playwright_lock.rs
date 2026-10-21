//! `.velnor/playwright.lock`: hash-pinned Playwright browser archives (hard decision 6).
//!
//! Written only by `velnor-actions playwright lock`. Playwright publishes no
//! checksums, so the first computation is trust-on-first-use of the download
//! host; every later install verifies the recorded SHA-256 and fails closed.
//! The `core_integrity` of each entry is the `playwright-core` tarball
//! integrity from `bun.lock`, the anchor of the browser revision table the
//! entry was derived from. See `docs/proposed/bun-contract.md`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// Repository-relative path of the committed browser lock.
pub const PLAYWRIGHT_LOCK_PATH: &str = ".velnor/playwright.lock";
/// Lock schema version.
pub const PLAYWRIGHT_LOCK_SCHEMA: u32 = 1;
/// Playwright host platform of the hosted Bun runner image (`ubuntu-26.04`, x86-64).
pub const PLAYWRIGHT_HOST_PLATFORM: &str = "ubuntu26.04-x64";
/// Hosts Playwright publishes browser archives on.
pub const PLAYWRIGHT_DOWNLOAD_HOSTS: [&str; 2] = [
    "cdn.playwright.dev",
    "playwright.download.prss.microsoft.com",
];
/// Playwright's `PLAYWRIGHT_CDN_MIRRORS`, in registry order
/// (`packages/playwright-core/src/server/registry/index.ts`, v1.64.1).
pub const PLAYWRIGHT_CDN_MIRRORS: [&str; 3] = [
    "https://cdn.playwright.dev/dbazure/download/playwright",
    "https://playwright.download.prss.microsoft.com/dbazure/download/playwright",
    "https://cdn.playwright.dev",
];
/// Path prefix whose archives Playwright serves from `cdn.playwright.dev` only (`cftUrl`).
const PLAYWRIGHT_SINGLE_MIRROR_PREFIX: &str = "builds/cft/";
/// Internal operation of the Playwright browser install step (HD-6).
pub const PLAYWRIGHT_INSTALL_OPERATION: &str = "playwright-browsers-install-v1";
/// Largest archive the install accepts (bytes).
pub const PLAYWRIGHT_MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

/// Parsed `.velnor/playwright.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaywrightLock {
    /// Lock schema; must equal [`PLAYWRIGHT_LOCK_SCHEMA`].
    pub schema: u32,
    /// Playwright host platform the archives were resolved for.
    pub platform: String,
    /// Pinned archives sorted by version, then directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archives: Vec<LockedArchive>,
}

/// One pinned browser archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedArchive {
    /// Playwright version (`playwright-core` in `bun.lock`).
    pub version: String,
    /// `playwright-core` tarball integrity recorded in `bun.lock`.
    pub core_integrity: String,
    /// Requested browsers that need this archive (`chromium`, `firefox`, `webkit`), sorted.
    pub browsers: Vec<String>,
    /// Directory name under the browsers path (`chromium-1248`).
    pub directory: String,
    /// Download URL on a Playwright host.
    pub url: String,
    /// Lowercase SHA-256 of the archive.
    pub sha256: String,
    /// Archive size in bytes.
    pub bytes: u64,
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// True for a Subresource-Integrity `sha512-` value.
#[must_use]
pub fn is_sha512_integrity(value: &str) -> bool {
    value.strip_prefix("sha512-").is_some_and(|b64| {
        b64.len() == 88
            && b64.ends_with("==")
            && b64
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    })
}

/// True for a browsers-path directory name such as `chromium_headless_shell-1248`.
#[must_use]
pub fn is_browser_directory(value: &str) -> bool {
    value.rsplit_once('-').is_some_and(|(name, revision)| {
        !name.is_empty()
            && name.len() <= 64
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            && !revision.is_empty()
            && revision.len() <= 12
            && revision.bytes().all(|b| b.is_ascii_digit())
    })
}

/// True for an https URL on a Playwright host naming a `.zip` with a plain path.
#[must_use]
pub fn is_playwright_download_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    PLAYWRIGHT_DOWNLOAD_HOSTS.contains(&host)
        && path.len() <= 256
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}

impl PlaywrightLock {
    /// Validate shape only.
    ///
    /// # Errors
    /// Names the lock path, key path and a machine-readable problem.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.schema != PLAYWRIGHT_LOCK_SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        let bad = |path: String, problem: &str| ContractError::config(file, path, problem);
        if self.platform != PLAYWRIGHT_HOST_PLATFORM {
            return Err(bad("platform".to_owned(), "unsupported_platform"));
        }
        let mut seen = BTreeSet::new();
        let mut previous: Option<(&str, &str)> = None;
        for (index, archive) in self.archives.iter().enumerate() {
            let path = format!("archives[{index}]");
            if !crate::config::is_exact_semver(&archive.version) {
                return Err(bad(format!("{path}.version"), "invalid_version"));
            }
            if !is_sha512_integrity(&archive.core_integrity) {
                return Err(bad(format!("{path}.core_integrity"), "invalid_integrity"));
            }
            let sorted_known = archive.browsers.windows(2).all(|pair| pair[0] < pair[1])
                && !archive.browsers.is_empty()
                && archive
                    .browsers
                    .iter()
                    .all(|name| matches!(name.as_str(), "chromium" | "firefox" | "webkit"));
            if !sorted_known {
                return Err(bad(format!("{path}.browsers"), "invalid_browsers"));
            }
            if !is_browser_directory(&archive.directory) {
                return Err(bad(format!("{path}.directory"), "invalid_directory"));
            }
            if !is_playwright_download_url(&archive.url) {
                return Err(bad(format!("{path}.url"), "invalid_url"));
            }
            if !is_lower_hex(&archive.sha256, 64) {
                return Err(bad(format!("{path}.sha256"), "invalid_sha256"));
            }
            if archive.bytes == 0 || archive.bytes > PLAYWRIGHT_MAX_ARCHIVE_BYTES {
                return Err(bad(format!("{path}.bytes"), "invalid_bytes"));
            }
            let key = (archive.version.as_str(), archive.directory.as_str());
            if !seen.insert(key) || previous.is_some_and(|last| last >= key) {
                return Err(bad(path, "unsorted_or_duplicate_archive"));
            }
            previous = Some(key);
        }
        Ok(())
    }

    /// The archives a member on `version` needs for `browsers`, sorted by directory.
    ///
    /// # Errors
    /// `playwright_lock_missing:<version>:<browser>` when a requested browser
    /// has no archive, and `playwright_lock_drift:<version>` when archives of
    /// one version disagree on the `bun.lock` integrity.
    pub fn archives_for(
        &self,
        version: &str,
        core_integrity: &str,
        browsers: &[&str],
    ) -> Result<Vec<&LockedArchive>, String> {
        for browser in browsers {
            if !self.archives.iter().any(|archive| {
                archive.version == version && archive.browsers.iter().any(|name| name == browser)
            }) {
                return Err(format!("playwright_lock_missing:{version}:{browser}"));
            }
        }
        let selected: Vec<&LockedArchive> = self
            .archives
            .iter()
            .filter(|archive| {
                archive.version == version
                    && archive
                        .browsers
                        .iter()
                        .any(|name| browsers.contains(&name.as_str()))
            })
            .collect();
        if selected
            .iter()
            .any(|archive| archive.core_integrity != core_integrity)
        {
            return Err(format!("playwright_lock_drift:{version}"));
        }
        Ok(selected)
    }
}

/// Return download URLs for a locked archive.
/// The locked URL comes first, followed by the other mirrors in Playwright registry order.
/// Archives under `builds/cft/` use only the locked URL.
/// The original URL is returned unchanged.
/// Additional URLs pass [`is_playwright_download_url`].
/// If the original URL does not match a known mirror, the function returns only that URL.
/// The caller's expected SHA-256 and size do not change with the host.
#[must_use]
pub fn playwright_download_candidates(url: &str) -> Vec<String> {
    let mut candidates = vec![url.to_owned()];
    let path = PLAYWRIGHT_CDN_MIRRORS.iter().find_map(|mirror| {
        url.strip_prefix(mirror)
            .and_then(|rest| rest.strip_prefix('/'))
    });
    let Some(path) = path else {
        return candidates;
    };
    if path.starts_with(PLAYWRIGHT_SINGLE_MIRROR_PREFIX) {
        return candidates;
    }
    for mirror in PLAYWRIGHT_CDN_MIRRORS {
        let candidate = format!("{mirror}/{path}");
        if !candidates.contains(&candidate) && is_playwright_download_url(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// The `directory:sha256` pin list the install step carries literally.
#[must_use]
pub fn pin_list(archives: &[&LockedArchive]) -> String {
    archives
        .iter()
        .map(|archive| format!("{}:{}", archive.directory, archive.sha256))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
#[path = "playwright_lock_tests.rs"]
mod tests;
