// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::load;
use crate::config::RunnerPlatform;
use crate::error::HostError;

const ARM64: u8 = 0xb7;
const AMD64: u8 = 0x3e;

fn elf(machine: u8, tail: u8) -> Vec<u8> {
    let mut bytes = vec![tail; 64];
    bytes[..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[18] = machine;
    bytes[19] = 0;
    bytes
}

fn write(dir: &Path, name: &str, bytes: &[u8], mode: u32) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("mode");
    path.to_str().expect("utf8").to_owned()
}

#[tokio::test]
async fn a_valid_executable_elf_for_the_platform_is_returned_as_bytes() {
    let dir = tempfile::tempdir().expect("dir");
    let bytes = elf(ARM64, 7);
    let path = write(dir.path(), "velnor-ci", &bytes, 0o755);
    assert_eq!(load(&path, RunnerPlatform::LinuxArm64).await, Ok(bytes));
}

#[tokio::test]
async fn the_file_is_read_again_on_every_call() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "velnor-ci", &elf(ARM64, 1), 0o755);
    let second = elf(ARM64, 2);
    assert_eq!(
        load(&path, RunnerPlatform::LinuxArm64).await,
        Ok(elf(ARM64, 1))
    );
    write(dir.path(), "velnor-ci", &second, 0o755);
    assert_eq!(load(&path, RunnerPlatform::LinuxArm64).await, Ok(second));
}

#[tokio::test]
async fn each_amd64_and_arm64_platform_accepts_only_its_own_machine() {
    let dir = tempfile::tempdir().expect("dir");
    let arm = write(dir.path(), "arm", &elf(ARM64, 0), 0o755);
    let amd = write(dir.path(), "amd", &elf(AMD64, 0), 0o755);
    assert!(load(&amd, RunnerPlatform::LinuxAmd64).await.is_ok());
    assert!(load(&arm, RunnerPlatform::LinuxArm64).await.is_ok());
    assert_eq!(
        load(&amd, RunnerPlatform::LinuxArm64).await,
        Err(HostError::Config)
    );
    assert_eq!(
        load(&arm, RunnerPlatform::LinuxAmd64).await,
        Err(HostError::Config)
    );
}

#[tokio::test]
async fn a_file_that_is_not_a_safe_executable_elf_is_refused() {
    let dir = tempfile::tempdir().expect("dir");
    let good = write(dir.path(), "good", &elf(ARM64, 0), 0o755);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&good, &link).expect("symlink");
    let mut macho = elf(ARM64, 0);
    macho[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    let mut big_endian = elf(ARM64, 0);
    big_endian[5] = 2;
    let mut class32 = elf(ARM64, 0);
    class32[4] = 1;
    let oversized = write(dir.path(), "big", &elf(ARM64, 0), 0o755);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&oversized)
        .and_then(|file| file.set_len((256 << 20) + 1))
        .expect("sparse");
    let cases = [
        link.to_str().expect("utf8").to_owned(),
        write(dir.path(), "plain", &elf(ARM64, 0), 0o644),
        write(dir.path(), "macho", &macho, 0o755),
        write(dir.path(), "be", &big_endian, 0o755),
        write(dir.path(), "c32", &class32, 0o755),
        write(dir.path(), "short", &[0x7f, b'E', b'L', b'F'], 0o755),
        oversized,
        dir.path().to_str().expect("utf8").to_owned(),
        dir.path()
            .join("missing")
            .to_str()
            .expect("utf8")
            .to_owned(),
        "relative/velnor-ci".to_owned(),
        String::new(),
    ];
    for case in cases {
        assert_eq!(
            load(&case, RunnerPlatform::LinuxArm64).await,
            Err(HostError::Config),
            "{case}"
        );
    }
}
