// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

#[path = "golden_collector_public/support.rs"]
mod support;

use support::{Harness, copy_fingerprint_inputs, retained_workspace};

#[test]
fn retains_nonzero_version_diagnostics_and_leaves_capture_destination_untouched()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let root_alias = harness.temp.path().join("repo symlink [x]*");
    symlink(&harness.root, &root_alias)?;
    let external_oracle = harness.temp.path().join("external oracle [x]*");
    let output = harness.run(
        "capture",
        &root_alias,
        &harness.guard_source,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&external_oracle),
    )?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("candidate --version command failed"),
        "unexpected collector diagnostic: {stderr}"
    );
    let workspace = retained_workspace(&output).ok_or("failed workspace was not retained")?;
    let raw = workspace.join("raw/version");
    let stdout = fs::read(raw.join("version.raw.txt"))?;
    let candidate_stderr = fs::read(raw.join("version.stderr.raw.txt"))?;
    let status = fs::read_to_string(raw.join("version.exit"))?;
    assert!(raw.join("version.raw.txt").is_file());
    assert!(raw.join("version.stderr.raw.txt").is_file());
    assert!(!stdout.is_empty() || !candidate_stderr.is_empty());
    assert_ne!(status, "0\n");
    assert!(!external_oracle.exists());
    Ok(())
}

#[test]
fn rejects_missing_candidate_and_wrong_guard_digest_before_capture() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let missing = harness.temp.path().join("missing candidate");
    let missing_output = harness.run(
        "check",
        &harness.root,
        &missing,
        &harness.archive_guard,
        &harness.guard_sha256,
        None,
    )?;
    assert_eq!(missing_output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_output.stderr).contains("No such file"));
    assert!(retained_workspace(&missing_output).is_none());

    let wrong_digest = if harness.guard_sha256.starts_with('0') {
        format!("1{}", &harness.guard_sha256[1..])
    } else {
        format!("0{}", &harness.guard_sha256[1..])
    };
    let digest_output = harness.run(
        "check",
        &harness.root,
        &harness.guard_source,
        &harness.archive_guard,
        &wrong_digest,
        None,
    )?;
    assert_eq!(digest_output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&digest_output.stderr).contains("SHA-256"),
        "unexpected guard digest diagnostic: {}",
        String::from_utf8_lossy(&digest_output.stderr)
    );
    assert!(retained_workspace(&digest_output).is_none());
    Ok(())
}

#[test]
fn rejects_source_changed_since_guard_compilation() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let fingerprint = Command::new(&harness.archive_guard)
        .arg("--fingerprint")
        .output()?;
    assert!(fingerprint.status.success());
    let fingerprint = String::from_utf8(fingerprint.stdout)?;
    let fingerprint = fingerprint.trim_end();
    let source = harness.directory.join("source snapshot");
    copy_fingerprint_inputs(&harness.root, &source)?;
    let initial =
        velnor_archive_guard::source_fingerprint(&source).map_err(std::io::Error::other)?;
    assert_eq!(initial, fingerprint);
    let manifest = source.join("Cargo.toml");
    let mut bytes = fs::read(&manifest)?;
    bytes.extend_from_slice(b"\n# test-only source fingerprint change\n");
    fs::write(manifest, bytes)?;
    let external_oracle = harness.temp.path().join("source-mismatch oracle");
    let output = harness.run(
        "check",
        &source,
        &harness.guard_source,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&external_oracle),
    )?;

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("source fingerprint is stale"),
        "unexpected source fingerprint diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(retained_workspace(&output).is_none());
    assert!(!external_oracle.exists());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn accepts_linux_elf_osabi_three_and_rejects_a_non_gnu_interpreter() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let linux_osabi = harness.copy_candidate("linux-osabi-three", |bytes| {
        assert!(bytes.starts_with(b"\x7fELF"));
        bytes[7] = 3;
    })?;
    let accepted = harness.run(
        "check",
        &harness.root,
        &linux_osabi,
        &harness.archive_guard,
        &harness.guard_sha256,
        None,
    )?;
    assert_eq!(accepted.status.code(), Some(2));
    let accepted_error = String::from_utf8_lossy(&accepted.stderr);
    assert!(accepted_error.contains("candidate --version command failed"));
    assert!(!accepted_error.contains("ELF ABI"));
    let workspace = retained_workspace(&accepted).ok_or("version failure lost its workspace")?;
    assert!(workspace.join("raw/version/version.exit").is_file());

    let bad_interpreter = harness.copy_candidate("bad-elf-interpreter", |bytes| {
        let interpreter = b"/lib64/ld-linux-x86-64.so.2\0";
        if let Some(offset) = bytes
            .windows(interpreter.len())
            .position(|window| window == interpreter)
        {
            bytes[offset] = b'x';
        }
    })?;
    let rejected = harness.run(
        "check",
        &harness.root,
        &bad_interpreter,
        &harness.archive_guard,
        &harness.guard_sha256,
        None,
    )?;
    assert_eq!(rejected.status.code(), Some(2));
    let rejected_error = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        rejected_error.contains("unexpected ELF interpreter"),
        "unexpected malformed ELF diagnostic: {rejected_error}"
    );
    assert!(retained_workspace(&rejected).is_none());
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn admits_both_supported_thin_macho_architectures_and_rejects_unknown_architecture()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let alternate_cpu = if cfg!(target_arch = "aarch64") {
        0x0100_0007_u32
    } else {
        0x0100_000c_u32
    };
    let alternate = harness.copy_candidate("alternate-macho-arch", |bytes| {
        assert_eq!(&bytes[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[4..8].copy_from_slice(&alternate_cpu.to_le_bytes());
    })?;
    let output = harness.run(
        "check",
        &harness.root,
        &alternate,
        &harness.archive_guard,
        &harness.guard_sha256,
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("unexpected Mach-O architecture"));

    let unsupported = harness.copy_candidate("unsupported-macho-arch", |bytes| {
        bytes[4..8].copy_from_slice(&0x0100_0001_u32.to_le_bytes());
    })?;
    let output = harness.run(
        "check",
        &harness.root,
        &unsupported,
        &harness.archive_guard,
        &harness.guard_sha256,
        None,
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unexpected Mach-O architecture"),
        "unexpected unsupported Mach-O diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
