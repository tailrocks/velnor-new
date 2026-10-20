use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use velnor_actions_contract::config::BunBrowser;

use super::*;

const LISTING: &str = "\
Chrome for Testing 156.0.8078.4 (playwright chromium v1248)
  Install location:    /Users/x/Library/Caches/ms-playwright/chromium-1248
  Download url:        https://cdn.playwright.dev/builds/cft/156.0.8078.4/linux64/chrome-linux64.zip

FFmpeg (playwright ffmpeg v1013)
  Install location:    /Users/x/Library/Caches/ms-playwright/ffmpeg-1013
  Download url:        https://cdn.playwright.dev/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-linux.zip
  Download fallback 1: https://playwright.download.prss.microsoft.com/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-linux.zip

Chrome Headless Shell 156.0.8078.4 (playwright chromium-headless-shell v1248)
  Install location:    /Users/x/Library/Caches/ms-playwright/chromium_headless_shell-1248
  Download url:        https://cdn.playwright.dev/builds/cft/156.0.8078.4/linux64/chrome-headless-shell-linux64.zip
";

const FIREFOX: &str = "\
Firefox 157.0 (playwright firefox v1555)
  Install location:    /c/ms-playwright/firefox-1555
  Download url:        https://cdn.playwright.dev/dbazure/download/playwright/builds/firefox/1555/firefox-ubuntu-24.04.zip
FFmpeg (playwright ffmpeg v1013)
  Install location:    /c/ms-playwright/ffmpeg-1013
  Download url:        https://cdn.playwright.dev/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-linux.zip
";

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

struct Fixture {
    core: Vec<u8>,
    listings: BTreeMap<&'static str, &'static str>,
    downloaded: RefCell<Vec<String>>,
}

impl BrowserSource for Fixture {
    fn download_core(&self, _version: &str, dest: &Path) -> Result<(), OrchestratorError> {
        fs::write(dest, &self.core).expect("core");
        Ok(())
    }

    fn dry_run(&self, core_dir: &Path, browser: &str) -> Result<String, OrchestratorError> {
        assert!(core_dir.join("cli.js").is_file(), "tarball extracted");
        Ok(self.listings[browser].to_owned())
    }

    fn download_archive(&self, url: &str, dest: &Path) -> Result<(), OrchestratorError> {
        self.downloaded.borrow_mut().push(url.to_owned());
        fs::write(dest, format!("archive:{url}")).expect("archive");
        Ok(())
    }
}

fn fixture(core: Vec<u8>) -> Fixture {
    Fixture {
        core,
        listings: BTreeMap::from([("chromium", LISTING), ("firefox", FIREFOX)]),
        downloaded: RefCell::new(Vec::new()),
    }
}

#[test]
fn the_dry_run_listing_yields_directories_and_primary_urls() {
    let found = parse_dry_run(LISTING).expect("parse");
    assert_eq!(
        found,
        [
            (
                "chromium-1248".to_owned(),
                "https://cdn.playwright.dev/builds/cft/156.0.8078.4/linux64/chrome-linux64.zip"
                    .to_owned()
            ),
            (
                "ffmpeg-1013".to_owned(),
                "https://cdn.playwright.dev/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-linux.zip"
                    .to_owned()
            ),
            (
                "chromium_headless_shell-1248".to_owned(),
                "https://cdn.playwright.dev/builds/cft/156.0.8078.4/linux64/chrome-headless-shell-linux64.zip"
                    .to_owned()
            ),
        ]
    );
    for bad in [
        "",
        "  Download url: https://cdn.playwright.dev/a.zip\n",
        "  Install location: /c/chromium-1\n  Download url: https://evil.example/a.zip\n",
        "  Install location: /c/../x\n  Download url: https://cdn.playwright.dev/a.zip\n",
        "  Install location: /c/chromium-1\n  Download url: http://cdn.playwright.dev/a.zip\n",
    ] {
        assert!(parse_dry_run(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_lock_merges_shared_archives_and_hashes_every_download() {
    let core = core_tarball();
    let source = fixture(core.clone());
    let scratch = tempfile::tempdir().expect("scratch");
    let scratch_path = scratch.path().canonicalize().expect("canonical");
    let requests = [CoreRequest {
        version: "1.64.1".to_owned(),
        integrity: integrity(&core),
        browsers: BTreeSet::from([BunBrowser::Chromium, BunBrowser::Firefox]),
    }];
    let lock = build_playwright_lock(&requests, &source, &scratch_path).expect("lock");
    let by_dir: BTreeMap<&str, &LockedArchive> = lock
        .archives
        .iter()
        .map(|archive| (archive.directory.as_str(), archive))
        .collect();
    assert_eq!(
        by_dir.keys().copied().collect::<Vec<_>>(),
        [
            "chromium-1248",
            "chromium_headless_shell-1248",
            "ffmpeg-1013",
            "firefox-1555"
        ]
    );
    assert_eq!(by_dir["ffmpeg-1013"].browsers, ["chromium", "firefox"]);
    assert_eq!(by_dir["firefox-1555"].browsers, ["firefox"]);
    let chromium = by_dir["chromium-1248"];
    assert_eq!(chromium.core_integrity, integrity(&core));
    assert_eq!(
        chromium.bytes,
        format!("archive:{}", chromium.url).len() as u64
    );
    let digest =
        hash_file(&scratch_path.join("core-1.64.1/chromium-1248.zip"), 1 << 20).expect("hash");
    assert_eq!(chromium.sha256, digest.0);
    assert_eq!(source.downloaded.borrow().len(), 4, "each archive once");
    // The document is deterministic and round-trips through the validator.
    let text = render_playwright_lock(&lock).expect("render");
    assert!(text.starts_with("# Generated by `velnor-actions playwright lock`"));
    let again: PlaywrightLock = toml::from_str(&text).expect("parse");
    assert_eq!(again, lock);
    again.validate(PLAYWRIGHT_LOCK_PATH).expect("valid");
}

#[test]
fn a_core_tarball_that_misses_the_bun_lock_integrity_is_never_run() {
    let core = core_tarball();
    let source = fixture(core);
    let scratch = tempfile::tempdir().expect("scratch");
    let scratch_path = scratch.path().canonicalize().expect("canonical");
    let requests = [CoreRequest {
        version: "1.64.1".to_owned(),
        integrity: integrity(b"something else"),
        browsers: BTreeSet::from([BunBrowser::Chromium]),
    }];
    let error = build_playwright_lock(&requests, &source, &scratch_path).expect_err("integrity");
    assert!(
        error
            .to_string()
            .contains("playwright_core_integrity_mismatch"),
        "{error}"
    );
    assert!(source.downloaded.borrow().is_empty());
}
