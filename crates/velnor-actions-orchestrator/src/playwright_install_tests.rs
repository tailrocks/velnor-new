use std::cell::Cell;
use std::fs;
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_actions_contract::config::playwright_lock::{
    LockedArchive, PLAYWRIGHT_HOST_PLATFORM, PLAYWRIGHT_LOCK_SCHEMA, PlaywrightLock,
};

use super::*;

const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const OTHER_INTEGRITY: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==";
const URL: &str = "https://cdn.playwright.dev/builds/cft/156.0.8078.4/linux64/chrome-linux64.zip";

fn bun_lock(version: &str, integrity: &str) -> String {
    format!(
        r#"{{"workspaces":{{"":{{"name":"r"}}}},"packages":{{"playwright-core":["playwright-core@{version}","",{{}},"{integrity}"]}}}}"#
    )
}

fn zip_bytes() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    writer
        .start_file("chrome-linux64/chrome", options)
        .expect("start");
    writer.write_all(b"#!/bin/sh\n").expect("write");
    writer.finish().expect("finish").into_inner()
}

fn sha(bytes: &[u8]) -> String {
    crate::config::playwright_lock::hex(&Sha256::digest(bytes))
}

fn archive(bytes: &[u8]) -> LockedArchive {
    LockedArchive {
        version: "1.64.1".to_owned(),
        core_integrity: INTEGRITY.to_owned(),
        browsers: vec!["chromium".to_owned()],
        directory: "chromium-1248".to_owned(),
        url: URL.to_owned(),
        sha256: sha(bytes),
        bytes: bytes.len() as u64,
    }
}

fn lock(bytes: &[u8]) -> PlaywrightLock {
    PlaywrightLock {
        schema: PLAYWRIGHT_LOCK_SCHEMA,
        platform: PLAYWRIGHT_HOST_PLATFORM.to_owned(),
        archives: vec![archive(bytes)],
    }
}

fn request(dir: &Path, bytes: &[u8]) -> InstallRequest {
    InstallRequest {
        version: "1.64.1".to_owned(),
        pins: vec![("chromium-1248".to_owned(), sha(bytes))],
        root: "frontend".to_owned(),
        workspace: dir.join("work"),
        runner_temp: dir.join("temp"),
        system_deps: Vec::new(),
    }
}

struct Fixture<'a> {
    served: &'a [u8],
    calls: Cell<usize>,
}

impl Downloader for Fixture<'_> {
    fn download(&self, url: &str, dest: &Path, _limit: u64) -> Result<(), OrchestratorError> {
        assert_eq!(url, URL);
        self.calls.set(self.calls.get() + 1);
        fs::write(dest, self.served).expect("serve");
        Ok(())
    }
}

#[test]
fn the_step_env_is_parsed_strictly() {
    let env = |extra: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = [
            ("VELNOR_PLAYWRIGHT_VERSION", "1.64.1"),
            ("VELNOR_PLAYWRIGHT_ROOT", "frontend"),
            (
                "VELNOR_PLAYWRIGHT_PINS",
                "chromium-1248:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
            ("GITHUB_WORKSPACE", "/w"),
            ("RUNNER_TEMP", "/t"),
        ]
        .iter()
        .chain(extra)
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
        move |key: &str| {
            owned
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    };
    assert!(InstallRequest::parse(&env(&[])).is_ok());
    for (key, value) in [
        ("VELNOR_PLAYWRIGHT_VERSION", "latest"),
        ("VELNOR_PLAYWRIGHT_ROOT", "../x"),
        ("VELNOR_PLAYWRIGHT_PINS", "chromium-1248:short"),
        (
            "VELNOR_PLAYWRIGHT_PINS",
            "../evil-1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ),
        ("RUNNER_TEMP", "relative"),
        ("GITHUB_WORKSPACE", ""),
    ] {
        assert!(
            InstallRequest::parse(&env(&[(key, value)])).is_err(),
            "{key}={value}"
        );
    }
}

#[test]
fn pins_must_match_the_lock_and_the_bun_lock_anchor() {
    let bytes = zip_bytes();
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let request = request(&root, &bytes);
    let lock = lock(&bytes);
    let anchor = bun_lock("1.64.1", INTEGRITY);
    assert_eq!(
        verified_archives(&request, Some(&lock), &anchor)
            .expect("ok")
            .len(),
        1
    );
    let fails = |lock: Option<&PlaywrightLock>, anchor: &str, code: &str| {
        let error = verified_archives(&request, lock, anchor).expect_err(code);
        assert!(error.to_string().contains(code), "{code}: {error}");
    };
    fails(None, &anchor, "playwright_lock_missing");
    fails(
        Some(&lock),
        &bun_lock("1.64.1", OTHER_INTEGRITY),
        "playwright_core_integrity_drift",
    );
    fails(
        Some(&lock),
        &bun_lock("1.63.0", INTEGRITY),
        "playwright_core_integrity_drift",
    );
    let mut edited = lock.clone();
    edited.archives[0].sha256 = sha(b"other");
    fails(Some(&edited), &anchor, "playwright_pin_drift:chromium-1248");
    let mut renamed = lock.clone();
    renamed.archives[0].directory = "chromium-9999".to_owned();
    fails(
        Some(&renamed),
        &anchor,
        "playwright_pin_drift:chromium-1248",
    );
}

#[test]
fn a_verified_archive_is_extracted_marked_and_reused() {
    let bytes = zip_bytes();
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let request = request(&root, &bytes);
    let lock = lock(&bytes);
    let archives: Vec<&LockedArchive> = lock.archives.iter().collect();
    let fixture = Fixture {
        served: &bytes,
        calls: Cell::new(0),
    };
    assert_eq!(install(&request, &archives, &fixture).expect("install"), 1);
    let installed = request.browsers_dir().join("chromium-1248");
    assert!(installed.join("chrome-linux64/chrome").is_file());
    assert!(installed.join("INSTALLATION_COMPLETE").is_file());
    assert_eq!(
        fs::read_to_string(installed.join("velnor-sha256")).expect("pin"),
        sha(&bytes)
    );
    assert!(
        !request
            .browsers_dir()
            .join("chromium-1248.partial")
            .exists()
    );
    assert!(!request.download_dir().join("chromium-1248.zip").exists());
    // A restored cache holding the pinned install downloads nothing.
    assert_eq!(install(&request, &archives, &fixture).expect("again"), 0);
    assert_eq!(fixture.calls.get(), 1);
    // A directory that does not carry the pin is replaced, never trusted.
    fs::write(installed.join("velnor-sha256"), "stale").expect("stale");
    fs::write(installed.join("injected"), "x").expect("injected");
    assert_eq!(install(&request, &archives, &fixture).expect("replace"), 1);
    assert!(!installed.join("injected").exists());
}

#[cfg(unix)]
#[test]
fn extraction_keeps_the_executable_bit() {
    use std::os::unix::fs::PermissionsExt as _;
    let bytes = zip_bytes();
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let request = request(&root, &bytes);
    let lock = lock(&bytes);
    let archives: Vec<&LockedArchive> = lock.archives.iter().collect();
    let fixture = Fixture {
        served: &bytes,
        calls: Cell::new(0),
    };
    install(&request, &archives, &fixture).expect("install");
    let mode = fs::metadata(
        request
            .browsers_dir()
            .join("chromium-1248/chrome-linux64/chrome"),
    )
    .expect("meta")
    .permissions()
    .mode();
    assert_eq!(mode & 0o111, 0o111);
}

#[test]
fn a_digest_or_size_mismatch_fails_closed_and_installs_nothing() {
    let bytes = zip_bytes();
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let request = request(&root, &bytes);
    let lock = lock(&bytes);
    let archives: Vec<&LockedArchive> = lock.archives.iter().collect();
    let mut tampered = bytes.clone();
    tampered.push(0);
    let fixture = Fixture {
        served: &tampered,
        calls: Cell::new(0),
    };
    let error = install(&request, &archives, &fixture).expect_err("mismatch");
    assert!(
        error
            .to_string()
            .contains("playwright_sha256_mismatch:chromium-1248"),
        "{error}"
    );
    assert!(!request.browsers_dir().join("chromium-1248").exists());
    assert!(
        !request
            .browsers_dir()
            .join("chromium-1248.partial")
            .exists()
    );
    assert!(!request.download_dir().join("chromium-1248.zip").exists());
}

#[test]
fn a_url_off_the_playwright_hosts_is_never_fetched() {
    let bytes = zip_bytes();
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let request = request(&root, &bytes);
    let mut rogue = archive(&bytes);
    rogue.url = "https://example.com/chrome-linux64.zip".to_owned();
    let fixture = Fixture {
        served: &bytes,
        calls: Cell::new(0),
    };
    let error = install(&request, &[&rogue], &fixture).expect_err("host");
    assert!(error.to_string().contains("playwright_url_host"), "{error}");
    assert_eq!(fixture.calls.get(), 0);
}

const FFMPEG_PRIMARY: &str =
    "https://cdn.playwright.dev/dbazure/download/playwright/builds/ffmpeg/1011/ffmpeg-linux.zip";
const FFMPEG_ESRP: &str = "https://playwright.download.prss.microsoft.com/dbazure/download/playwright/builds/ffmpeg/1011/ffmpeg-linux.zip";
const FFMPEG_BUCKET: &str = "https://cdn.playwright.dev/builds/ffmpeg/1011/ffmpeg-linux.zip";

enum Host {
    Dns,
    ServerError,
    Serve(Vec<u8>),
}

/// Scripted per-host transport: DNS failure, 5xx, or a body, keyed by URL.
struct Scripted {
    hosts: Vec<(&'static str, Host)>,
    seen: std::cell::RefCell<Vec<String>>,
}

impl Downloader for Scripted {
    fn download(&self, url: &str, dest: &Path, _limit: u64) -> Result<(), OrchestratorError> {
        self.seen.borrow_mut().push(url.to_owned());
        match self.hosts.iter().find(|(known, _)| *known == url) {
            Some((_, Host::Serve(body))) => {
                fs::write(dest, body).expect("serve");
                Ok(())
            }
            Some((_, Host::Dns | Host::ServerError)) | None => {
                Err(internal("playwright_download_failed"))
            }
        }
    }
}

fn ffmpeg_setup(bytes: &[u8]) -> (tempfile::TempDir, InstallRequest, LockedArchive) {
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let mut locked = archive(bytes);
    locked.directory = "ffmpeg-1011".to_owned();
    locked.url = FFMPEG_PRIMARY.to_owned();
    let mut req = request(&root, bytes);
    req.pins = vec![("ffmpeg-1011".to_owned(), sha(bytes))];
    (dir, req, locked)
}

#[test]
fn candidates_follow_the_registry_order_and_cft_has_no_fallback() {
    assert_eq!(
        playwright_download_candidates(FFMPEG_PRIMARY),
        [FFMPEG_PRIMARY, FFMPEG_ESRP, FFMPEG_BUCKET]
    );
    assert_eq!(
        playwright_download_candidates(FFMPEG_BUCKET),
        [FFMPEG_BUCKET, FFMPEG_PRIMARY, FFMPEG_ESRP]
    );
    assert_eq!(playwright_download_candidates(URL), [URL]);
}

#[test]
fn a_dead_primary_falls_back_to_the_next_mirror() {
    let bytes = zip_bytes();
    let (_dir, req, locked) = ffmpeg_setup(&bytes);
    let scripted = Scripted {
        hosts: vec![
            (FFMPEG_PRIMARY, Host::ServerError),
            (FFMPEG_ESRP, Host::Dns),
            (FFMPEG_BUCKET, Host::Serve(bytes.clone())),
        ],
        seen: std::cell::RefCell::new(Vec::new()),
    };
    assert_eq!(install(&req, &[&locked], &scripted).expect("install"), 1);
    assert_eq!(
        *scripted.seen.borrow(),
        [FFMPEG_PRIMARY, FFMPEG_ESRP, FFMPEG_BUCKET]
    );
    assert!(req.browsers_dir().join("ffmpeg-1011").is_dir());
}

#[test]
fn a_mirror_serving_the_wrong_hash_is_rejected() {
    let bytes = zip_bytes();
    let (_dir, req, locked) = ffmpeg_setup(&bytes);
    let mut wrong = bytes.clone();
    wrong.push(0);
    let scripted = Scripted {
        hosts: vec![
            (FFMPEG_PRIMARY, Host::Dns),
            (FFMPEG_ESRP, Host::Serve(wrong.clone())),
            (FFMPEG_BUCKET, Host::Serve(wrong)),
        ],
        seen: std::cell::RefCell::new(Vec::new()),
    };
    let error = install(&req, &[&locked], &scripted).expect_err("rejected");
    assert!(
        error
            .to_string()
            .contains("playwright_sha256_mismatch:ffmpeg-1011"),
        "{error}"
    );
    assert!(!req.browsers_dir().join("ffmpeg-1011").exists());
    assert!(!req.download_dir().join("ffmpeg-1011.zip").exists());
}

#[test]
fn all_hosts_failing_reports_the_download_failure() {
    let bytes = zip_bytes();
    let (_dir, req, locked) = ffmpeg_setup(&bytes);
    let scripted = Scripted {
        hosts: vec![],
        seen: std::cell::RefCell::new(Vec::new()),
    };
    let error = install(&req, &[&locked], &scripted).expect_err("failed");
    assert!(
        error.to_string().contains("playwright_download_failed"),
        "{error}"
    );
    assert_eq!(scripted.seen.borrow().len(), 3);
}
