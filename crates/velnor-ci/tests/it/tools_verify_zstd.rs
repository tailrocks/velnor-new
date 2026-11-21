// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::tools_verify::{Job, job, recorded, rustc};
use crate::tools_verify_notice::{GZIP_VERSION, ZSTD_VERSION, check_stdout};

fn fake_zstd(dir: &Path, ran: &Path) {
    fs::create_dir_all(dir).unwrap_or_else(|e| unreachable!("{e}"));
    let zstd = dir.join("zstd");
    fs::write(
        &zstd,
        format!(
            "#!/bin/sh\necho ran > '{}'\necho '*** Zstandard CLI v1.5.5'\n",
            ran.display()
        ),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&zstd, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn notice_with_path(job: &Job, path: &str) -> String {
    let anchor = recorded(job);
    fs::write(rustc(job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    check_stdout(
        job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("PATH", path),
        ],
    )
}

fn names_version(stdout: &str, version: &str) -> bool {
    stdout.contains(&format!("--key 'velnor-tools-abc123' --version {version}"))
}

fn dir(job: &Job, name: &str) -> PathBuf {
    job.root.join(name)
}

#[test]
fn a_zstd_in_a_safe_path_directory_is_used_for_the_version() {
    let job = job("zstd-safe-path");
    let ran = dir(&job, "zstd-ran");
    let bin = dir(&job, "zstd-bin");
    fake_zstd(&bin, &ran);
    let stdout = notice_with_path(&job, &bin.display().to_string());
    assert!(ran.exists(), "{stdout}");
    assert!(names_version(&stdout, ZSTD_VERSION), "{stdout}");
}

#[test]
fn a_zstd_in_a_relative_path_entry_is_refused() {
    let job = job("zstd-relative-path");
    let ran = dir(&job, "zstd-ran");
    fake_zstd(&dir(&job, "zstd-bin"), &ran);
    let stdout = notice_with_path(&job, ":zstd-bin");
    assert!(!ran.exists(), "{stdout}");
    assert!(names_version(&stdout, GZIP_VERSION), "{stdout}");
}

#[test]
fn a_zstd_in_a_world_writable_path_directory_is_refused() {
    let job = job("zstd-world-writable");
    let ran = dir(&job, "zstd-ran");
    let bin = dir(&job, "zstd-bin");
    fake_zstd(&bin, &ran);
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o777))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = notice_with_path(&job, &bin.display().to_string());
    assert!(!ran.exists(), "{stdout}");
    assert!(names_version(&stdout, GZIP_VERSION), "{stdout}");
}

#[test]
fn a_zstd_inside_the_restored_tools_is_never_run() {
    let job = job("zstd-in-restored-tools");
    let ran = dir(&job, "zstd-ran");
    let shims = job.home.join(".local/share/mise/shims");
    fake_zstd(&shims, &ran);
    let stdout = notice_with_path(&job, &shims.display().to_string());
    assert!(!ran.exists(), "{stdout}");
    assert!(names_version(&stdout, GZIP_VERSION), "{stdout}");
}

#[test]
fn a_symlinked_path_directory_into_the_restored_tools_is_never_run() {
    let job = job("zstd-linked-into-restored-tools");
    let ran = dir(&job, "zstd-ran");
    let shims = job.home.join(".rustup/shims");
    fake_zstd(&shims, &ran);
    let link = dir(&job, "linked-bin");
    std::os::unix::fs::symlink(&shims, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = notice_with_path(&job, &link.display().to_string());
    assert!(!ran.exists(), "{stdout}");
    assert!(names_version(&stdout, GZIP_VERSION), "{stdout}");
}
