// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::sha256_current_executable;
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
use sha2::{Digest, Sha256};
use std::{error::Error, fs::File, io::Write, os::fd::AsRawFd, process::Command, time::Duration};
use velnor_actions_mise::CheckDeadline;

const CHILD_MODE: &str = "VELNOR_SELFHASH_MEMFD_CHILD";
const MAX_BYTES: &str = "VELNOR_SELFHASH_MEMFD_MAX_BYTES";
const TEST_NAME: &str =
    "homebrew::sdk_binding::assets::memfd_tests::selfhash_reads_the_loaded_sealed_executable";

#[cfg(target_os = "linux")]
#[test]
fn selfhash_reads_the_loaded_sealed_executable() -> Result<(), Box<dyn Error>> {
    if std::env::var_os(CHILD_MODE).is_some() {
        let max_bytes = std::env::var(MAX_BYTES)?.parse()?;
        let digest =
            sha256_current_executable(CheckDeadline::after(Duration::from_secs(10))?, max_bytes)?;
        println!("SELFHASH={digest}");
        return Ok(());
    }
    let bytes = std::fs::read(std::env::current_exe()?)?;
    let expected = hex(&Sha256::digest(&bytes));
    let fd = memfd_create("velnor-selfhash", MemfdFlags::ALLOW_SEALING)?;
    let mut executable = File::from(fd);
    executable.write_all(&bytes)?;
    fcntl_add_seals(
        &executable,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )?;
    let child = Command::new(format!("/proc/self/fd/{}", executable.as_raw_fd()))
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(CHILD_MODE, "1")
        .env(MAX_BYTES, bytes.len().to_string())
        .output()?;
    assert!(
        child.status.success(),
        "memfd child failed: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    let stdout = String::from_utf8(child.stdout)?;
    assert!(
        stdout
            .lines()
            .any(|line| line == format!("SELFHASH={expected}"))
    );
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
