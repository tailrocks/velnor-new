use super::*;

fn integrity() -> String {
    format!("sha512-{}==", "A".repeat(86))
}

fn archive(version: &str, directory: &str, browsers: &[&str]) -> LockedArchive {
    LockedArchive {
        version: version.to_owned(),
        core_integrity: integrity(),
        browsers: browsers.iter().map(|name| (*name).to_owned()).collect(),
        directory: directory.to_owned(),
        url: format!("https://cdn.playwright.dev/builds/{directory}.zip"),
        sha256: "a".repeat(64),
        bytes: 10,
    }
}

fn lock(archives: Vec<LockedArchive>) -> PlaywrightLock {
    PlaywrightLock {
        schema: PLAYWRIGHT_LOCK_SCHEMA,
        platform: PLAYWRIGHT_HOST_PLATFORM.to_owned(),
        archives,
    }
}

type Mutation = Box<dyn Fn(&mut LockedArchive)>;

fn problem(lock: &PlaywrightLock) -> String {
    lock.validate(PLAYWRIGHT_LOCK_PATH)
        .expect_err("invalid")
        .to_string()
}

#[test]
fn a_well_formed_lock_validates_and_selects_by_browser() {
    let lock = lock(vec![
        archive("1.64.1", "chromium-1248", &["chromium"]),
        archive("1.64.1", "ffmpeg-1013", &["chromium", "firefox"]),
        archive("1.64.1", "firefox-1555", &["firefox"]),
        archive("1.65.0", "chromium-1300", &["chromium"]),
    ]);
    lock.validate(PLAYWRIGHT_LOCK_PATH).expect("valid");
    let chromium = lock
        .archives_for("1.64.1", &integrity(), &["chromium"])
        .expect("chromium");
    assert_eq!(
        chromium
            .iter()
            .map(|a| a.directory.as_str())
            .collect::<Vec<_>>(),
        ["chromium-1248", "ffmpeg-1013"]
    );
    assert_eq!(
        pin_list(&chromium),
        format!(
            "chromium-1248:{},ffmpeg-1013:{}",
            "a".repeat(64),
            "a".repeat(64)
        )
    );
    let both = lock
        .archives_for("1.64.1", &integrity(), &["chromium", "firefox"])
        .expect("both");
    assert_eq!(both.len(), 3);
    assert_eq!(
        lock.archives_for("1.64.1", &integrity(), &["webkit"]),
        Err("playwright_lock_missing:1.64.1:webkit".to_owned())
    );
    assert_eq!(
        lock.archives_for("1.63.0", &integrity(), &["chromium"]),
        Err("playwright_lock_missing:1.63.0:chromium".to_owned())
    );
    let other = format!("sha512-{}==", "B".repeat(86));
    assert_eq!(
        lock.archives_for("1.64.1", &other, &["chromium"]),
        Err("playwright_lock_drift:1.64.1".to_owned())
    );
}

#[test]
fn malformed_entries_are_rejected_by_field() {
    let good = || archive("1.64.1", "chromium-1248", &["chromium"]);
    let cases: [(&str, Mutation); 8] = [
        ("version", Box::new(|a| a.version = "latest".to_owned())),
        (
            "core_integrity",
            Box::new(|a| a.core_integrity = "sha1-x".to_owned()),
        ),
        (
            "browsers",
            Box::new(|a| a.browsers = vec!["opera".to_owned()]),
        ),
        ("directory", Box::new(|a| a.directory = "../x-1".to_owned())),
        (
            "url",
            Box::new(|a| a.url = "https://evil.example/a.zip".to_owned()),
        ),
        (
            "url",
            Box::new(|a| a.url = "http://cdn.playwright.dev/a.zip".to_owned()),
        ),
        ("sha256", Box::new(|a| a.sha256 = "ABC".to_owned())),
        ("bytes", Box::new(|a| a.bytes = 0)),
    ];
    for (field, mutate) in cases {
        let mut entry = good();
        mutate(&mut entry);
        let text = problem(&lock(vec![entry]));
        assert!(
            text.contains(&format!("archives[0].{field}")),
            "{field}: {text}"
        );
    }
    let mut wrong = lock(vec![good()]);
    wrong.platform = "ubuntu24.04-x64".to_owned();
    assert!(problem(&wrong).contains("unsupported_platform"));
    let mut schema = lock(vec![good()]);
    schema.schema = 2;
    assert!(schema.validate(PLAYWRIGHT_LOCK_PATH).is_err());
    let unsorted = lock(vec![
        archive("1.64.1", "ffmpeg-1013", &["chromium"]),
        good(),
    ]);
    assert!(problem(&unsorted).contains("unsorted_or_duplicate_archive"));
    let repeated = lock(vec![good(), good()]);
    assert!(problem(&repeated).contains("unsorted_or_duplicate_archive"));
}

#[test]
fn url_and_directory_grammars_are_closed() {
    for url in [
        "https://cdn.playwright.dev/builds/cft/156.0.8078.4/linux64/chrome-linux64.zip",
        "https://playwright.download.prss.microsoft.com/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-linux.zip",
    ] {
        assert!(is_playwright_download_url(url), "{url}");
    }
    for url in [
        "https://cdn.playwright.dev/a.tar.gz",
        "https://cdn.playwright.dev/../a.zip",
        "https://cdn.playwright.dev/a.zip?x=1",
        "https://user@cdn.playwright.dev/a.zip",
        "https://cdn.playwright.dev.evil.example/a.zip",
        "https://cdn.playwright.dev/",
    ] {
        assert!(!is_playwright_download_url(url), "{url}");
    }
    assert!(is_browser_directory("chromium_headless_shell-1248"));
    for name in ["chromium", "-1", "Chromium-1", "../a-1", "a-b", "a-"] {
        assert!(!is_browser_directory(name), "{name}");
    }
}
