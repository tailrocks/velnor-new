//! Committed-manifest file-read tests.
//!
//! Declared via `#[path]` from `discover.rs` under `cfg(test)` so the
//! discovery module keeps the file size gate. The helper is
//! cfg-independent, so these debug-mode tests cover the exact read the
//! release twin relies on.

use std::fs;

use tempfile::TempDir;

use super::read_manifest_file;

/// Present file returns its exact text.
#[test]
fn present_file_returns_its_text() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("release-manifest.json"), "{\"schema\":1}").expect("manifest");
    assert_eq!(
        read_manifest_file(root.path()),
        Some("{\"schema\":1}".to_owned())
    );
}

/// Absent file returns `None` (release builds fail closed on this).
#[test]
fn absent_file_returns_none() {
    let root = TempDir::new().expect("temp root");
    assert_eq!(read_manifest_file(root.path()), None);
    fs::create_dir_all(root.path().join(".velnor")).expect("velnor dir");
    assert_eq!(read_manifest_file(root.path()), None);
}

/// Non-UTF-8 file returns `None` instead of lossy text.
#[test]
fn non_utf8_file_returns_none() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("release-manifest.json"), [0xff, 0xfe]).expect("manifest");
    assert_eq!(read_manifest_file(root.path()), None);
}
