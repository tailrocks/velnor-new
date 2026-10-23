use std::cell::RefCell;
use std::io::Write;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha512};

use super::*;

const CORE_URL: &str = "https://registry.npmjs.org/playwright-core/-/playwright-core-1.64.1.tgz";

fn core_tarball() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let body = b"console.log('cli');\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "package/cli.js", &body[..])
        .expect("append");
    let tar_bytes = builder.into_inner().expect("tar");
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&tar_bytes).expect("gz");
    gz.finish().expect("finish")
}

fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", STANDARD.encode(Sha512::digest(bytes)))
}

fn archive(browser: &str, core_integrity: &str) -> LockedArchive {
    LockedArchive {
        version: "1.64.1".to_owned(),
        core_integrity: core_integrity.to_owned(),
        browsers: vec![browser.to_owned()],
        directory: format!("{browser}-2370"),
        url: format!("https://cdn.playwright.dev/builds/{browser}.zip"),
        sha256: "a".repeat(64),
        bytes: 1,
    }
}

fn request(temp: &Path, system_deps: &[&str]) -> InstallRequest {
    InstallRequest {
        version: "1.64.1".to_owned(),
        pins: vec![("webkit-2370".to_owned(), "a".repeat(64))],
        root: "frontend".to_owned(),
        workspace: temp.join("work"),
        runner_temp: temp.join("temp"),
        system_deps: system_deps.iter().map(|name| (*name).to_owned()).collect(),
    }
}

struct Serve {
    bytes: Vec<u8>,
    urls: RefCell<Vec<String>>,
}

impl Downloader for Serve {
    fn download(&self, url: &str, dest: &Path, limit: u64) -> Result<(), OrchestratorError> {
        assert_eq!(limit, CORE_MAX_BYTES);
        self.urls.borrow_mut().push(url.to_owned());
        fs::write(dest, &self.bytes).expect("serve");
        Ok(())
    }
}

#[derive(Default)]
struct Record {
    calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
}

impl DepsInstaller for Record {
    fn install_deps(&self, cli: &Path, browsers: &[String]) -> Result<(), OrchestratorError> {
        assert!(cli.is_file(), "the verified cli.js is extracted first");
        self.calls
            .borrow_mut()
            .push((cli.to_path_buf(), browsers.to_vec()));
        Ok(())
    }
}

#[test]
fn the_browser_list_is_closed_sorted_and_unique() {
    assert_eq!(
        parse_system_deps(None).expect("absent"),
        Vec::<String>::new()
    );
    assert_eq!(parse_system_deps(Some("webkit")).expect("one"), ["webkit"]);
    assert_eq!(
        parse_system_deps(Some("firefox,webkit")).expect("two"),
        ["firefox", "webkit"]
    );
    for bad in ["", "chromium", "webkit,firefox", "webkit,webkit", "webkit,"] {
        assert!(parse_system_deps(Some(bad)).is_err(), "{bad}");
    }
}

#[test]
fn install_deps_runs_only_the_integrity_verified_core() {
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let core = core_tarball();
    let anchor = integrity(&core);
    let webkit = archive("webkit", &anchor);
    let serve = Serve {
        bytes: core,
        urls: RefCell::new(Vec::new()),
    };
    let installer = Record::default();
    install_system_deps(&request(&root, &["webkit"]), &[&webkit], &serve, &installer)
        .expect("installed");
    assert_eq!(*serve.urls.borrow(), [CORE_URL]);
    let calls = installer.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].0.ends_with("extracted/package/cli.js"),
        "{calls:?}"
    );
    assert_eq!(calls[0].1, ["webkit"]);
}

#[test]
fn a_tampered_core_or_an_unpinned_browser_never_runs_install_deps() {
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().canonicalize().expect("canonical");
    let core = core_tarball();
    let other = format!("sha512-{}==", "B".repeat(86));
    let installer = Record::default();
    let serve = Serve {
        bytes: core.clone(),
        urls: RefCell::new(Vec::new()),
    };
    let error = install_system_deps(
        &request(&root, &["webkit"]),
        &[&archive("webkit", &other)],
        &serve,
        &installer,
    )
    .expect_err("tampered");
    assert!(
        error
            .to_string()
            .contains("playwright_core_integrity_mismatch"),
        "{error}"
    );
    let error = install_system_deps(
        &request(&root, &["firefox"]),
        &[&archive("webkit", &integrity(&core))],
        &serve,
        &installer,
    )
    .expect_err("unpinned");
    assert!(
        error
            .to_string()
            .contains("playwright_system_deps_unpinned:firefox"),
        "{error}"
    );
    assert_eq!(serve.urls.borrow().len(), 1, "unpinned never downloads");
    assert_eq!(installer.calls.borrow().len(), 0);
    install_system_deps(&request(&root, &[]), &[], &serve, &installer).expect("none");
    assert_eq!(installer.calls.borrow().len(), 0);
}

#[test]
fn the_step_env_carries_the_system_deps() {
    let env = |deps: Option<&'static str>| {
        move |key: &str| match key {
            "VELNOR_PLAYWRIGHT_VERSION" => Some("1.64.1".to_owned()),
            "VELNOR_PLAYWRIGHT_ROOT" => Some("frontend".to_owned()),
            "VELNOR_PLAYWRIGHT_PINS" => Some(format!("webkit-2370:{}", "a".repeat(64))),
            "GITHUB_WORKSPACE" => Some("/w".to_owned()),
            "RUNNER_TEMP" => Some("/t".to_owned()),
            "VELNOR_PLAYWRIGHT_SYSTEM_DEPS" => deps.map(str::to_owned),
            _ => None,
        }
    };
    assert_eq!(
        InstallRequest::parse(&env(None))
            .expect("absent")
            .system_deps,
        Vec::<String>::new()
    );
    assert_eq!(
        InstallRequest::parse(&env(Some("webkit")))
            .expect("webkit")
            .system_deps,
        ["webkit"]
    );
    assert!(InstallRequest::parse(&env(Some("chromium"))).is_err());
}

/// Scripted `install-deps` outcomes for [`install_with_retry`].
fn scripted(
    outcomes: Vec<Result<bool, velnor_actions_mise::MiseError>>,
) -> (Result<(), OrchestratorError>, usize) {
    let mut outcomes = outcomes.into_iter();
    let mut runs = 0;
    let result = install_with_retry(2, &mut || {
        runs += 1;
        outcomes.next().expect("scripted outcome")
    });
    (result, runs)
}

fn spawn_failed(message: &str) -> velnor_actions_mise::MiseError {
    velnor_actions_mise::MiseError::SpawnFailed {
        program: "mise".to_owned(),
        message: message.to_owned(),
    }
}

#[test]
fn a_first_success_runs_once_and_a_failure_then_success_runs_twice() {
    let (result, runs) = scripted(vec![Ok(true), Ok(true)]);
    assert!(result.is_ok());
    assert_eq!(runs, 1);
    let (result, runs) = scripted(vec![Ok(false), Ok(true)]);
    assert!(result.is_ok());
    assert_eq!(runs, 2);
}

#[test]
fn two_failures_stop_at_two_with_the_last_error() {
    let (result, runs) = scripted(vec![Ok(false), Err(spawn_failed("timeout_after_secs:480"))]);
    assert_eq!(runs, 2);
    let error = result.expect_err("two failures").to_string();
    assert!(error.contains("timeout_after_secs:480"), "{error}");
}

#[test]
fn a_cleaned_up_timeout_retries_and_a_failed_cleanup_never_does() {
    let (result, runs) = scripted(vec![Err(spawn_failed("timeout_after_secs:480")), Ok(true)]);
    assert!(result.is_ok());
    assert_eq!(runs, 2);
    let (result, runs) = scripted(vec![
        Err(spawn_failed(
            "timeout_after_secs:480;cleanup_failed:reap: EPERM",
        )),
        Ok(true),
    ]);
    assert_eq!(runs, 1, "the previous process group may still run");
    let error = result.expect_err("cleanup failure is terminal").to_string();
    assert!(error.contains(";cleanup_failed:reap: EPERM"), "{error}");
}
