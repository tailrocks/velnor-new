// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;

use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use velnor_actions_orchestrator::verify_crate_archive;

#[test]
fn crate_entries_are_bound_to_approved_tracked_source() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, process::Command};
    use velnor_actions_orchestrator::verify_archive_tracked_content;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let git = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let out = Command::new("git").current_dir(root).args(args).output()?;
        if !out.status.success() {
            return Err("git fixture failed".into());
        }
        Ok(String::from_utf8(out.stdout)?.trim().to_owned())
    };
    git(&["init", "-q"])?;
    git(&["config", "user.name", "Alexey Zhokhov"])?;
    git(&["config", "user.email", "alexey@zhokhov.com"])?;
    let manifest = b"[package]\nname = \"demo\"\nversion = \"0.1.1\"\n";
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), b"pub fn good() {}")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("lib.rs", root.join("src/link.rs"))?;
    git(&["add", "."])?;
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "fixture",
    ])?;
    let sha = git(&["rev-parse", "HEAD"])?;
    let vcs = format!(r#"{{"git":{{"sha1":"{sha}"}},"path_in_vcs":""}}"#);
    let make = |lib: &[u8], vcs: &str| {
        let mut files: Vec<(&str, &[u8])> = vec![
            ("demo-0.1.1/Cargo.toml", manifest),
            ("demo-0.1.1/Cargo.toml.orig", manifest),
            ("demo-0.1.1/.cargo_vcs_info.json", vcs.as_bytes()),
            ("demo-0.1.1/src/lib.rs", lib),
        ];
        if cfg!(unix) {
            files.push(("demo-0.1.1/src/link.rs", b"pub fn good() {}"));
        }
        archive(&files)
    };
    verify_archive_tracked_content(
        &make(b"pub fn good() {}", &vcs)?,
        root,
        &sha,
        "Cargo.toml",
        "demo",
        "0.1.1",
    )?;
    assert!(
        verify_archive_tracked_content(
            &make(b"pub fn evil() {}", &vcs)?,
            root,
            &sha,
            "Cargo.toml",
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    assert!(
        verify_archive_tracked_content(
            &make(b"pub fn good() {}", &vcs.replace(&sha, &"a".repeat(40)))?,
            root,
            &sha,
            "Cargo.toml",
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    #[cfg(unix)]
    reject_archive_symlink_targets(root, &git, &make)?;
    Ok(())
}

#[cfg(unix)]
fn reject_archive_symlink_targets(
    root: &std::path::Path,
    git: &impl Fn(&[&str]) -> Result<String, Box<dyn std::error::Error>>,
    make: &impl Fn(&[u8], &str) -> Result<Vec<u8>, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use velnor_actions_orchestrator::verify_archive_tracked_content;
    let outside = tempfile::NamedTempFile::new()?;
    fs::remove_file(root.join("src/link.rs"))?;
    std::os::unix::fs::symlink(outside.path(), root.join("src/link.rs"))?;
    git(&["add", "src/link.rs"])?;
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "escaping link",
    ])?;
    let escaped_sha = git(&["rev-parse", "HEAD"])?;
    let escaped_vcs = format!(r#"{{"git":{{"sha1":"{escaped_sha}"}},"path_in_vcs":""}}"#);
    assert!(
        verify_archive_tracked_content(
            &make(b"pub fn good() {}", &escaped_vcs)?,
            root,
            &escaped_sha,
            "Cargo.toml",
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    fs::remove_file(root.join("src/link.rs"))?;
    fs::write(root.join("src/untracked.rs"), b"pub fn good() {}")?;
    std::os::unix::fs::symlink("untracked.rs", root.join("src/link.rs"))?;
    git(&["add", "src/link.rs"])?;
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "untracked target",
    ])?;
    let untracked_sha = git(&["rev-parse", "HEAD"])?;
    let untracked_vcs = format!(r#"{{"git":{{"sha1":"{untracked_sha}"}},"path_in_vcs":""}}"#);
    assert!(
        verify_archive_tracked_content(
            &make(b"pub fn good() {}", &untracked_vcs)?,
            root,
            &untracked_sha,
            "Cargo.toml",
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    Ok(())
}

fn archive(files: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    archive_mode(files, 0o644)
}

fn archive_mode(files: &[(&str, &[u8])], mode: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        tar.append_data(&mut header, *name, *bytes)?;
    }
    let encoder = tar.into_inner()?;
    Ok(encoder.finish()?)
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut result = String::new();
    let alphabet = b"0123456789abcdef";
    for byte in Sha256::digest(bytes) {
        result.push(char::from(alphabet[usize::from(byte >> 4)]));
        result.push(char::from(alphabet[usize::from(byte & 15)]));
    }
    result
}

#[test]
fn published_crate_matches_expected_files_and_registry_checksum()
-> Result<(), Box<dyn std::error::Error>> {
    let expected = archive(&[
        ("demo-0.1.1/Cargo.toml", b"[package]"),
        ("demo-0.1.1/src/lib.rs", b"pub fn test() {}"),
    ])?;
    let actual = archive(&[
        ("demo-0.1.1/src/lib.rs", b"pub fn test() {}"),
        ("demo-0.1.1/Cargo.toml", b"[package]"),
    ])?;
    let checksum = hex_digest(&actual);
    verify_crate_archive(&expected, &actual, "demo", "0.1.1", &checksum)?;
    let wrong_mode = archive_mode(
        &[
            ("demo-0.1.1/Cargo.toml", b"[package]"),
            ("demo-0.1.1/src/lib.rs", b"pub fn test() {}"),
        ],
        0o755,
    )?;
    let wrong_mode_checksum = hex_digest(&wrong_mode);
    assert!(
        verify_crate_archive(
            &expected,
            &wrong_mode,
            "demo",
            "0.1.1",
            &wrong_mode_checksum
        )
        .is_err()
    );
    let wrong = archive(&[
        ("demo-0.1.1/Cargo.toml", b"[package]"),
        ("demo-0.1.1/src/lib.rs", b"malicious"),
    ])?;
    let wrong_checksum = hex_digest(&wrong);
    assert!(verify_crate_archive(&expected, &wrong, "demo", "0.1.1", &wrong_checksum).is_err());
    assert!(verify_crate_archive(&expected, &actual, "demo", "0.1.1", &"0".repeat(64)).is_err());
    let duplicate = archive(&[
        ("demo-0.1.1/Cargo.toml", b"[package]"),
        ("demo-0.1.1/src/lib.rs", b"pub fn test() {}"),
        ("demo-0.1.1/src/lib.rs", b"replacement"),
    ])?;
    let duplicate_checksum = hex_digest(&duplicate);
    assert!(
        verify_crate_archive(&expected, &duplicate, "demo", "0.1.1", &duplicate_checksum).is_err()
    );
    let mut invalid = actual.clone();
    invalid.write_all(b"trailing")?;
    let invalid_checksum = hex_digest(&invalid);
    assert!(verify_crate_archive(&expected, &invalid, "demo", "0.1.1", &invalid_checksum).is_err());
    Ok(())
}
