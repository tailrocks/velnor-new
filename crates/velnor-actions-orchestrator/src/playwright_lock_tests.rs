use velnor_actions_contract::config::playwright_lock::{
    LockedArchive, PLAYWRIGHT_HOST_PLATFORM, PLAYWRIGHT_LOCK_SCHEMA,
};

use super::*;

#[test]
fn a_lock_renders_deterministically_and_reads_back_validated() {
    let lock = PlaywrightLock {
        schema: PLAYWRIGHT_LOCK_SCHEMA,
        platform: PLAYWRIGHT_HOST_PLATFORM.to_owned(),
        archives: vec![LockedArchive {
            version: "1.64.1".to_owned(),
            core_integrity: format!("sha512-{}==", "A".repeat(86)),
            browsers: vec!["chromium".to_owned()],
            directory: "chromium-1248".to_owned(),
            url: "https://cdn.playwright.dev/builds/cft/1/linux64/chrome-linux64.zip".to_owned(),
            sha256: "a".repeat(64),
            bytes: 12,
        }],
    };
    let text = render_playwright_lock(&lock).expect("render");
    assert_eq!(text, render_playwright_lock(&lock).expect("again"));
    let dir = tempfile::tempdir().expect("dir");
    assert!(read_playwright_lock(dir.path()).expect("absent").is_none());
    fs::create_dir_all(dir.path().join(".velnor")).expect("velnor");
    fs::write(dir.path().join(PLAYWRIGHT_LOCK_PATH), &text).expect("write");
    assert_eq!(read_playwright_lock(dir.path()).expect("read"), Some(lock));
    fs::write(
        dir.path().join(PLAYWRIGHT_LOCK_PATH),
        text.replace("chromium-1248", "../x-1"),
    )
    .expect("tamper");
    assert!(read_playwright_lock(dir.path()).is_err());
}

#[test]
fn files_are_hashed_in_chunks_with_a_size_bound() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("blob");
    fs::write(&path, b"abc").expect("blob");
    let (sha, bytes) = hash_file(&path, 10).expect("hash");
    assert_eq!(
        sha,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(bytes, 3);
    assert!(hash_file(&path, 2).is_err());
}
